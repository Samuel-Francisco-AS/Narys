//! Single composition root for domain services. Presentation clients own no runtime.
use crate::{
    agents::registry::AgentRegistry,
    cognition::{registry::ProviderRegistry, ProviderRuntime},
    execution::ExecutionBroker,
    luna::runtime::TaskRegistry,
    persistence::database::Database,
};
use narys_domain::security::secrets::SecretStore;
use std::{path::PathBuf, sync::Arc};
pub struct RuntimeServices {
    pub secrets: Arc<SecretStore>,
    pub tasks: Arc<TaskRegistry>,
    pub providers: Arc<ProviderRuntime>,
    pub database: Database,
    pub sessions: narys_domain::cognition::sessions::CurrentRunSessions,
    pub conversation_admission: tokio::sync::Mutex<()>,
    pub agents: AgentRegistry,
    pub execution: Arc<ExecutionBroker>,
    pub copilot: Arc<crate::copilot::CopilotLifecycle>,
}
impl RuntimeServices {
    pub fn new(database: Database, secret_directory: PathBuf) -> Result<Self, &'static str> {
        Self::with_copilot_cli(
            database,
            secret_directory,
            PathBuf::from("/nonexistent-copilot-core-config-required"),
        )
    }
    pub fn with_copilot_cli(
        database: Database,
        secret_directory: PathBuf,
        cli: PathBuf,
    ) -> Result<Self, &'static str> {
        let db = database.open().map_err(|e| e.code())?;
        let tasks = Arc::new(TaskRegistry::default());
        tasks.seed_next_id(crate::persistence::task_history::max_id(&db).map_err(|e| e.code())?);
        let secrets = Arc::new(SecretStore::existing(secret_directory));
        let mut registry = ProviderRegistry::default();
        use crate::cognition::{cloudflare::*, gemini::*, groq::*, mistral::*, types::*};
        let groq = Arc::new(
            GroqProvider::new(GroqConfig::default(), secrets.clone())
                .map_err(|_| "groq_http_client_unavailable")?,
        );
        let gemini = Arc::new(
            GeminiProvider::new(GeminiConfig::default(), secrets.clone())
                .map_err(|_| "gemini_http_client_unavailable")?,
        );
        let cloudflare = Arc::new(
            CloudflareProvider::new(CloudflareConfig::default(), secrets.clone())
                .map_err(|_| "cloudflare_http_client_unavailable")?,
        );
        let mistral = Arc::new(
            MistralProvider::new(MistralConfig::default(), secrets.clone())
                .map_err(|_| "mistral_http_client_unavailable")?,
        );
        for (id, handle) in [
            ("groq", groq.timeout_handle()),
            ("gemini", gemini.timeout_handle()),
            ("cloudflare", cloudflare.timeout_handle()),
            ("mistral", mistral.timeout_handle()),
        ] {
            *handle.write().map_err(|_| "provider_timeout_lock_failed")? =
                crate::persistence::provider_timeouts::load(&db, id).map_err(|e| e.code())?;
        }
        for (id, priority, capabilities, provider) in [
            (
                "groq",
                1,
                ProviderCapabilities::with_structured_output(),
                groq as Arc<dyn crate::cognition::provider::Provider>,
            ),
            ("gemini", 2, ProviderCapabilities::text_stream(), gemini),
            ("mistral", 3, ProviderCapabilities::text_stream(), mistral),
            (
                "cloudflare",
                4,
                ProviderCapabilities::text_stream(),
                cloudflare,
            ),
        ] {
            registry.register(
                ProviderConfig {
                    id: id.into(),
                    enabled: true,
                    priority,
                    capabilities,
                },
                provider,
            )?;
        }
        let providers = Arc::new(
            ProviderRuntime::with_database(registry, database.clone()).map_err(|e| e.code())?,
        );
        providers.connect_credentials(&secrets);
        let root = database
            .directory()
            .parent()
            .ok_or("agent_state_directory_missing")?
            .join("copilot");
        crate::server::mkdir(&root)?;
        let copilot = crate::copilot::CopilotLifecycle::new(
            database.clone(),
            tasks.clone(),
            root.join("sessions"),
            Arc::new(crate::copilot::sdk::SdkRuntimeFactory::new(
                cli,
                root.join("runtimes"),
                database.clone(),
            )),
        )?;
        let mut agents = AgentRegistry::production();
        agents.register(
            crate::copilot::CopilotAgentAdapter::config(),
            Arc::new(crate::copilot::CopilotAgentAdapter {
                lifecycle: copilot.clone(),
            }),
        )?;
        Ok(Self {
            secrets,
            tasks,
            providers,
            database,
            sessions: Default::default(),
            conversation_admission: tokio::sync::Mutex::new(()),
            agents,
            copilot,
            execution: ExecutionBroker::process_wide(),
        })
    }
    pub fn shutdown(&self) {
        self.copilot.request_shutdown();
        self.tasks.shutdown();
        self.execution.request_shutdown();
    }
}

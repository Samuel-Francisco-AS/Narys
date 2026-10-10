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
    pub providers: ProviderRuntime,
    pub agents: AgentRegistry,
    pub execution: Arc<ExecutionBroker>,
}
impl RuntimeServices {
    pub fn new(database: Database, secret_directory: PathBuf) -> Result<Self, &'static str> {
        let db = database.open().map_err(|e| e.code())?;
        let tasks = Arc::new(TaskRegistry::default());
        tasks.seed_next_id(crate::persistence::task_history::max_id(&db).map_err(|e| e.code())?);
        let secrets = Arc::new(SecretStore::existing(secret_directory));
        Ok(Self {
            secrets,
            tasks,
            // Provider wiring and Conversation IPC are SERVER-1B. An empty real
            // registry is explicit; no mock or provider process is started at boot.
            providers: ProviderRuntime::with_database(ProviderRegistry::default(), database)
                .map_err(|e| e.code())?,
            agents: AgentRegistry::default(),
            execution: ExecutionBroker::process_wide(),
        })
    }
    pub fn shutdown(&self) {
        self.tasks.shutdown();
        self.execution.request_shutdown();
    }
}

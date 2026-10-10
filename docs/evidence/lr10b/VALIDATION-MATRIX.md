# Matriz de evidências LR-10B

Candidata para auditoria. SDK_FIXTURE usa **o SDK Rust oficial** e subprocessos Linux reais, com peer JSON-RPC sintético local; não usa o CLI Copilot real nem inferência. MOCK testa interleavings por semáforos determinísticos. REAL_IPC executa o binário Core e socket Unix reais com HOME isolado.

| # | Propriedade | Evidência | Testes / limites |
|---|---|---|---|
| 1 | Core ocioso, sem residente | REAL_HOST + REAL_IPC | host-before/after; idle_queries_never_start_and_first_demand_starts; specialist_queries_disconnect_reentry_and_restart_are_lazy_and_durable |
| 2 | Primeira demanda | SDK_FIXTURE + MOCK | official_sdk_fixture_creates_detaches_resumes_and_never_sends; idle_queries_never_start_and_first_demand_starts |
| 3 | Startup simultâneo | MOCK | simultaneous_demands_share_startup_leases_and_last_release_stops |
| 4 | Leases e última liberação | MOCK | simultaneous_demands_share_startup_leases_and_last_release_stops; cancellation_of_waiting_lease_does_not_stop_other_work |
| 5 | Cancelar inicialização | MOCK + SDK_FIXTURE | cancellation_during_startup_cleans_without_restart_or_session; sdk_cancel_during_create_aborts_detaches_and_reaps |
| 6 | Cancelar atividade | MOCK + SDK_FIXTURE | cancellation_during_activity_and_dropped_client_future_are_cleaned; sdk_cancel_during_create_aborts_detaches_and_reaps |
| 7 | Corrida cancelamento/conclusão | MOCK | cancel_completion_race_is_terminal_once_and_no_implicit_restart (32 corridas); startup_failure_is_not_hidden_by_concurrent_cancellation |
| 8 | Shutdown concorrente/forçado | MOCK + SDK_FIXTURE + REAL_IPC | concurrent_shutdown_cancels_drains_and_closes_admission; graceful_deadline_forces_owned_runtime_and_records_verified_recovery; specialist_maintenance_stop_is_typed_and_keeps_conversation_available |
| 9 | Falha/morte de subprocesso | SDK_FIXTURE | unexpected_cli_death_is_reaped_and_recovery_is_explicit; killed_guardian_and_killed_primary_each_recover_descendants_without_touching_external |
| 10 | CLI ausente/pin/versão | SDK_FIXTURE + REAL_IPC | cli_absent_and_wrong_pin_do_not_launch_runtime; incompatible_runtime_is_cleaned_before_admission; cli_symlink_and_fifo_are_rejected_without_blocking |
| 11 | Observer/atraso/gaps | MOCK + SDK_FIXTURE | progress_observation_failure_is_a_gap_not_a_functional_failure; actual_failure_is_preserved_when_observation_is_unavailable; detached_observer_and_burst_events_never_leak_private_payload; late_tool_and_error_events_are_real_failures_not_observer_failures |
| 12 | Desconexão/reentrada cliente | REAL_IPC + MOCK | specialist_queries_disconnect_reentry_and_restart_are_lazy_and_durable; detached_clients_do_not_cancel_task_and_durable_resume_is_explicit. Logout SSH físico durante Copilot real: NOT_VERIFIED |
| 13 | Restart e recovery | REAL_IPC + SDK_FIXTURE + MOCK | restart_recovers_uncertain_state_without_remote_replay_or_id_collision; restart_reconciles_owned_artifacts_without_relaunching_or_signalling_external; resume_requires_matching_durable_history_anchor_without_create_fallback |
| 14 | Ferramentas não autorizadas | SDK_FIXTURE + MOCK + REAL_IPC | official_deny_all_handler_rejects_native_shell_permission_request; adapter_and_registry_keep_tools_financial_gate_and_codex_planner_closed; typed_protocol_bounds_versions_and_future_authority_are_enforced |
| 15 | Codex Planner read-only | DOMAIN_REGRESSION + MOCK | adapter_and_registry_keep_tools_financial_gate_and_codex_planner_closed; regressão agents::codex::* (gates reais ignorados) |
| 16 | Conversation/Scheduler/IPC | DOMAIN_REGRESSION + REAL_IPC | regressão cognition::*; conversation_sessions_product_errors_and_free_permission_are_real_ipc; official_cli_*; control |
| 17 | Órfãos/recursos | SDK_FIXTURE + REAL_HOST | recibos kernel_children_exhausted, identidade /proc ausente, filho setsid e processo externo preservado; interrupted_startup_and_artifact_retention_are_bounded_and_recoverable; RSS/CPU/cgroup ocioso. Endurance e consumo de inferência real: NOT_VERIFIED |

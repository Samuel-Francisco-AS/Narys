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

## LR-10B FIX-1 — Startup Ownership & Recovery Safety

**CANDIDATA CORRIGIDA, AGUARDANDO REAUDITORIA INDEPENDENTE DA LUNA.**
A matriz original acima registra o histórico; esta seção substitui a evidência de
startup/ownership/recovery contestada pela auditoria. As provas novas executam o
SDK oficial com peer sintético e verificam memória, SQLite e processos Linux reais.
Nenhuma inferência/CLI autenticado é usada. Logs: [fix1/core-tests.txt](fix1/core-tests.txt).

| # | Cenário exigido | Teste(s) | Invariável durável / efeitos |
|---|---|---|---|
| 1 | workspace após INSERT | preparation_workspace_logs_sdk_state_failures_are_durable_no_launch_and_retry_safe | stopped, verified=1; failed_before_launch; no_process_launched; SDK zero |
| 2 | logs após INSERT | preparation_workspace_logs_sdk_state_failures_are_durable_no_launch_and_retry_safe | mesmo invariável; fixture cria arquivo bloqueante real |
| 3 | sdk-state após INSERT | preparation_workspace_logs_sdk_state_failures_are_durable_no_launch_and_retry_safe | mesmo invariável; diretório compartilhado de estado reparado apenas na fixture |
| 4 | persistência startup | insert_persistence_failure_blocks_memory_and_reconciles_empty_durable_intent_without_launch; launch_intent_and_terminal_persistence_faults_never_bless_cleanup; silently_ignored_safety_write_is_a_persistence_failure_not_successful_recovery | PersistenceUncertain; memória/run verified=0; estado durável último commit ou faulted; zero linhas não libera recovery |
| 5 | imediatamente antes de launch | failure_immediately_before_sdk_invocation_concludes_durable_intent_as_no_launch | launch_intent observado, depois failed_before_launch/stopped/verified=1 por prova de não invocação |
| 6 | pós-launch com prova | launched_failure_requires_kernel_proof_and_restart_never_replays_or_signals_external | failed_after_launch/stopped/verified=1; kernel_children_exhausted; IDs ausentes; erro original preservado |
| 7 | pós-launch sem prova | launched_unverified_cleanup_blocks_new_generations_until_explicit_positive_recovery | faulted/verified=0 na memória, owner e run; recibo contraditório impede reentrada |
| 8 | nova demanda recuperada | preparation_workspace_logs_sdk_state_failures_are_durable_no_launch_and_retry_safe; terminal_cleanup_bit_without_valid_evidence_cannot_bypass_reconciliation | nova geração somente após certificado positivo; um create para a demanda nova, nenhum retry do run antigo |
| 9 | demanda bloqueada | launched_unverified_cleanup_blocks_new_generations_until_explicit_positive_recovery; lost_ownership_row_with_retained_artifacts_blocks_launch_and_recovery | memória bloqueia; factory nova também verifica autoridade/artifacts; nenhum novo runtime/session SDK |
| 10 | restart pré-launch | restart_of_committed_preparation_recovers_no_launch_and_run_ids_without_replay; interrupted_journal_drop_is_faulted_and_prelaunch_recovery_is_positive_and_idempotent | preparing positivo → stopped/verified=1; run777 interrupted; TaskId novo maior; nenhum create durante recovery |
| 11 | restart pós-launch | launched_failure_requires_kernel_proof_and_restart_never_replays_or_signals_external; operational_and_cleanup_persistence_failures_preserve_original_error_and_require_reconciliation | estado terminal/runs preservados; proof/persistência reconciliados; erro original não vira sucesso |
| 12 | recovery repetido | incomplete_legacy_and_invalid_boot_records_stay_blocking_but_previous_boot_recovers; crash_at_durable_launch_intent_without_cleanup_is_never_guessed_as_prelaunch; testes acima chamam recovery duas vezes | idempotência; bootUUID válido distinto é prova, inválido não; intenção sem recibo permanece incerta |
| 13 | cancelamento × falha | cancel_racing_before_and_after_launch_preserves_original_failure_and_durable_safety | task failed único com erro original; prova pré/pós-launch explícita; generation=1, nenhum restart |
| 14 | processo externo | contradictory_preparation_and_external_pid_are_never_adopted_or_signalled; launched_failure_requires_kernel_proof_and_restart_never_replays_or_signals_external | sleep externo vivo após duas recoveries; descendente setsid próprio reaped; somente teste encerra seu sleep |
| 15 | sem duplicação | restart_of_committed_preparation_recovers_no_launch_and_run_ids_without_replay; launched_unverified_cleanup_blocks_new_generations_until_explicit_positive_recovery; preparation_workspace_logs_sdk_state_failures_are_durable_no_launch_and_retry_safe | counts/distinct TaskId/sessions/owners conferidos; create somente explícito; send=0 em todos os peers |

A prova adicional startup_safety_is_evidence_typed_and_never_classified_by_error_identity
usa o mesmo código sanitizado com quatro classes e verifica o run SQLite e a memória.
terminal_cleanup_bit_without_valid_evidence_cannot_bypass_reconciliation verifica
também status: bit registrado=1 não é apresentado como cleanup comprovado quando o
certificado falta. Zero linhas afetadas/falha de persistência mantém a recuperação
bloqueada, mesmo se o SQLite ainda conserva um bit antigo não confiável.

17 testes novos (16 cenários SDK/fault-injection + 1 contrato mock), com subcasos.
Regressão final: Core: 81 unitários +11 integrações; Domain: 1059+2 doctests, dois gates
reais ignorados; Python: 17. Contagens não substituem os invariáveis acima.

Restart pré-launch é uma fixture de crash sobre journal já commitado, sem invocar
SDK; restart/recovery do controlador é reconstruído com a mesma autoridade. Não
se afirma ter matado o Core instalado dentro de uma inicialização Copilot real.
NOT_VERIFIED anteriores de CLI autenticado, inferência, billing, SSH físico,
endurance e MSRV exato permanecem. Verificar host-before/after e host-update em fix1
para a instalação real ociosa; ela não executou agent new/resume.

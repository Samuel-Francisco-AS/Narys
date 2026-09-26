# LR-4 — identidade e memória locais

O Luna Core Rust mantém `luna.sqlite3` em `app_local_data_dir` do Tauri. O SQLite é a autoridade local após a importação inicial; React recebe apenas comandos específicos e metadados seguros. Notion pode ser origem ou backup externo, mas não participa do runtime. Nenhuma LLM é usada nesta etapa.

## Dados

- `identity_snapshots`: versões estruturadas da identidade. Cada versão tem `version` única, `supersedes_id` e um índice parcial que admite apenas uma linha `is_current=1`. Versões antigas permanecem no banco.
- `memory_records`: sínteses úteis para continuidade operacional, com tipo, domínios, estado, importância, confiança, proveniência contextual e possível `supersedes_id`. Uma `import_key` única evita duplicação de fontes externas.
- `conversation_sessions` e `conversation_messages`: sessões e mensagens locais; a conversa desta etapa é artificial e diagnóstica.
- `task_records`: um resumo por tarefa mock terminal, sem persistir o stream de eventos.

A migration SQL `001_initial_persistence.sql` é aplicada em transação e controlada por `PRAGMA user_version`. Conexões ativam `foreign_keys=ON` e timeout de 3 segundos. O volume pequeno dispensa pool; cada operação abre uma conexão dentro de `spawn_blocking`. O banco usa o journal padrão do SQLite, sem WAL nesta fase.

## Importação e recuperação

Em `tauri dev`, o comando de diagnóstico importa `src-tauri/private/luna-bootstrap.private.json`. O arquivo é opcional e ignorado pelo Git. O parser `serde` valida a estrutura e os valores antes de abrir a transação. `identity.version` e `memory.import_key` tornam a repetição idempotente. A leitura normal consulta apenas SQLite, mesmo que o JSON seja removido depois.

O Core expõe internamente `current_identity()` e filtros determinísticos para memórias ativas por tipo, domínio, importância mínima e limite. A ordenação inicial é importância decrescente, data de evento decrescente. Memória é contexto revisável, não verdade absoluta. Registros superados permanecem como histórico. Não se armazenam cadeias de pensamento privadas; as memórias devem ser sínteses deliberadas. Embeddings, RAG e busca semântica ficam fora desta etapa.

Um `JournalEntry` futuro corresponde ao Caderno da Luna: observações, perspectivas, hipóteses e perguntas abertas. Seu papel difere da memória operacional, e o Caderno não é importado nem misturado automaticamente com `MemoryRecord`.

## Privacidade e segurança

O bootstrap real, o banco e seus sidecars não são versionados. Testes usam exclusivamente dados sintéticos. A UI não mostra o JSON de identidade, e logs não registram conteúdo de memórias ou conversas. Erros enviados à WebView são códigos controlados, sem SQL ou caminhos privados.

**O SQLite não é criptografado integralmente na LR-4.** Ele contém identidade, memória, conversa e histórico, mas nenhuma API key ou credencial. Segredos continuam responsabilidade exclusiva do `SecretStore` Stronghold. A limitação conhecida da chave de desbloqueio Stronghold permanece como gate obrigatório antes da LR-6.

O Context Builder poderá, em etapa posterior, usar a identidade atual e poucas memórias selecionadas por esses filtros. Ele não existe nesta LR-4.

#!/usr/bin/env python3
"""Compare the candidate to its authorized base; no remote/provider calls."""
import subprocess
from pathlib import Path
BASE = '46da34e0cf316fb11f4ed01a0f516b627641991f'
def original(path):
    return subprocess.check_output(['git','show',f'{BASE}:{path}'],text=True)
def function(source, name):
    start=source.index(f'fn {name}(')
    return source[start:source.index('\n}',start)+2]
checks = {
 'src-tauri/src/luna/runtime.rs':['chat_budget_and_request'],
 'src-tauri/src/cognition/summary.rs':['summary_request','summary_input'],
 'src-tauri/src/cognition/orchestrator.rs':['model_contract','task_graph_model_contract','request','task_graph_request','request_with_contract'],
 'src-tauri/src/cognition/task_graph_worker.rs':['worker_input','worker_system_instruction'],
 'src-tauri/src/agents/codex/backend.rs':['thread_config','thread_start_params','turn_start_params','production_config','inspect_notification','inspect_notification_mode'],
 'src-tauri/src/agents/planner.rs':['output_schema'],
}
for path,names in checks.items():
    before=original(path);after=Path(path).read_text()
    for name in names:
        assert function(before,name)==function(after,name), f'Prompt/security contract changed: {name}'
    if path.endswith('codex/backend.rs'):
        assert next(l for l in before.splitlines() if l.startswith('const STATIC_INSTRUCTIONS'))==next(l for l in after.splitlines() if l.startswith('const STATIC_INSTRUCTIONS'))
for path in ['src-tauri/src/lib.rs','src-tauri/build.rs','src-tauri/tauri.conf.json','src-tauri/Cargo.toml','src-tauri/Cargo.lock','package.json','package-lock.json','src-tauri/src/cognition/types.rs','src-tauri/src/agents/types.rs']:
    assert original(path)==Path(path).read_text(),f'Authority/dependency/request contract changed: {path}'
for prefix in ['src/','src-tauri/capabilities/','src-tauri/permissions/','src-tauri/migrations/','src-tauri/src/execution/','src-tauri/src/security/']:
    assert not subprocess.check_output(['git','diff','--name-only',BASE,'--',prefix],text=True).strip(),f'Out-of-scope change: {prefix}'
adapters='\n'.join(p.read_text() for p in Path('src-tauri/src/operational_trace/adapters').glob('*.rs') if p.name!='tests.rs')
for forbidden in ['ProviderRequest','ContextBundle','format!("{:?}"','reasoning/textDelta','turn/start','internal_system_instruction']:
    assert forbidden not in adapters, f'Forbidden adapter dependency/content: {forbidden}'
assert 'ReasoningText' not in adapters and 'ChainOfThought' not in adapters
print('LR-9D security: unchanged prompts/schemas/security protocol; zero commands/permissions/capabilities/migrations/dependencies/frontend/execution authority changes')

CREATE TABLE cognitive_continuations (
 root_task_id INTEGER PRIMARY KEY CHECK(root_task_id BETWEEN 1 AND 9007199254740991),
 manifest_json TEXT NOT NULL CHECK(length(CAST(manifest_json AS BLOB)) BETWEEN 1 AND 49152),
 state TEXT NOT NULL CHECK(state IN ('running','paused','completed','cancelled','failed')),
 pause_reason TEXT CHECK(pause_reason IN ('economic_authorization','recovery_required','uncertain_execution','insufficient_durable_context','invalid_recovery')),
 generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 9007199254740991),
 CHECK((state='paused') = (pause_reason IS NOT NULL))
);
CREATE TABLE cognitive_continuation_units (
 root_task_id INTEGER NOT NULL REFERENCES cognitive_continuations(root_task_id),
 subtask_id TEXT NOT NULL CHECK(length(subtask_id) BETWEEN 1 AND 64),
 unit_sequence INTEGER CHECK(unit_sequence BETWEEN 1 AND 9007199254740991),
 state TEXT NOT NULL CHECK(state IN ('not_started','started','completed')),
 allocation_json TEXT CHECK(length(CAST(allocation_json AS BLOB)) BETWEEN 1 AND 4096),
 selection_json TEXT CHECK(length(CAST(selection_json AS BLOB)) BETWEEN 1 AND 512),
 budget_json TEXT CHECK(length(CAST(budget_json AS BLOB)) BETWEEN 1 AND 256),
 result_json TEXT CHECK(length(CAST(result_json AS BLOB)) BETWEEN 1 AND 102400),
 checkpoint_sequence INTEGER CHECK(checkpoint_sequence BETWEEN 1 AND 9007199254740991),
 PRIMARY KEY(root_task_id,subtask_id), UNIQUE(root_task_id,unit_sequence),
 CHECK((state='not_started' AND unit_sequence IS NULL AND allocation_json IS NULL AND selection_json IS NULL AND budget_json IS NULL AND result_json IS NULL AND checkpoint_sequence IS NULL)
 OR (state='started' AND unit_sequence IS NOT NULL AND allocation_json IS NOT NULL AND selection_json IS NOT NULL AND budget_json IS NOT NULL AND result_json IS NULL AND checkpoint_sequence IS NULL)
 OR (state='completed' AND unit_sequence IS NOT NULL AND allocation_json IS NOT NULL AND selection_json IS NOT NULL AND budget_json IS NOT NULL AND result_json IS NOT NULL AND checkpoint_sequence IS NOT NULL))
);

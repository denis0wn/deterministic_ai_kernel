PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA temp_store=MEMORY;
PRAGMA mmap_size=268435456;
PRAGMA busy_timeout=5000;

CREATE TABLE IF NOT EXISTS generations (
    id INTEGER PRIMARY KEY AUTOINCREMENT
);

CREATE TABLE IF NOT EXISTS event_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    system_generation BIGINT NOT NULL,
    causal_unit_id BIGINT NOT NULL,
    sequence_in_unit INTEGER NOT NULL,
    task_id TEXT NOT NULL,
    step_id TEXT,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    logical_generation BIGINT NOT NULL,
    UNIQUE(causal_unit_id, sequence_in_unit)
);

CREATE TABLE IF NOT EXISTS effect_ledger (
    effect_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    reservation_generation BIGINT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('reserved','committed','rejected'))
);

CREATE TABLE IF NOT EXISTS external_effects (
    effect_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    observed_state TEXT NOT NULL CHECK(observed_state IN ('committed','rejected')),
    executed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    result_payload TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS leases (
    lease_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    worker_id TEXT NOT NULL,
    acquired_generation BIGINT NOT NULL,
    expires_at_generation BIGINT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active','expired','released'))
);

CREATE TABLE IF NOT EXISTS state_snapshots (
    snapshot_id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    last_generation BIGINT NOT NULL,
    payload TEXT NOT NULL
);



CREATE TABLE IF NOT EXISTS replay_capsules (
    capsule_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    payload TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS semantic_artifacts (
    artifact_id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    source_generation BIGINT NOT NULL,
    artifact_type TEXT NOT NULL CHECK(artifact_type IN ('analysis_seed','retrieval_result','classification','semantic_bias_v1')),
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS tasks (
    task_id TEXT PRIMARY KEY,
    task_class TEXT NOT NULL CHECK(task_class IN ('Generic','PlannerHardening','CodeFix'))
);

CREATE TABLE IF NOT EXISTS step_dependencies (
    task_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    depends_on_step_id TEXT NOT NULL,
    PRIMARY KEY(task_id, step_id, depends_on_step_id)
);

CREATE TABLE IF NOT EXISTS step_status (
    task_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending','ready','dispatched','committed','rejected')),
    PRIMARY KEY(task_id, step_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_effect_task_step
ON effect_ledger(task_id, step_id);

CREATE UNIQUE INDEX IF NOT EXISTS uq_active_lease
ON leases(task_id, step_id)
WHERE state = 'active';

CREATE INDEX IF NOT EXISTS idx_event_task
ON event_log(task_id, causal_unit_id, sequence_in_unit);

CREATE INDEX IF NOT EXISTS idx_effect_task
ON effect_ledger(task_id, step_id);

CREATE INDEX IF NOT EXISTS idx_external_effect_task
ON external_effects(task_id, step_id);

CREATE INDEX IF NOT EXISTS idx_lease_task
ON leases(task_id, step_id, state);

CREATE INDEX IF NOT EXISTS idx_snapshot_task
ON state_snapshots(task_id, snapshot_id);



CREATE INDEX IF NOT EXISTS idx_replay_capsules_task
ON replay_capsules(task_id, created_at);

CREATE INDEX IF NOT EXISTS idx_semantic_artifacts_task
ON semantic_artifacts(task_id, step_id, source_generation);

CREATE INDEX IF NOT EXISTS idx_deps_task
ON step_dependencies(task_id, step_id, depends_on_step_id);

CREATE INDEX IF NOT EXISTS idx_status_task
ON step_status(task_id, step_id, status);

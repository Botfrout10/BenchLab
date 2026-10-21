-- BenchLab initial schema, based on SPEC.md §11.2.
-- Deviation: environments.archived_at is included because FR-ENV-01 requires
-- archive as a first-class environment action. Archived environments keep all
-- data and are hidden from the default list.

CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  accent TEXT NOT NULL DEFAULT 'blue',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_name
  ON projects(lower(name)) WHERE archived_at IS NULL;

CREATE TABLE IF NOT EXISTS environments (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('local','staging','production')),
  base_url TEXT NOT NULL,
  variables_json TEXT NOT NULL DEFAULT '[]',
  auth_json TEXT NOT NULL DEFAULT '{"kind":"none"}',
  tls_verify INTEGER NOT NULL DEFAULT 1,
  allow_external_hosts INTEGER NOT NULL DEFAULT 0,
  allowed_hosts_json TEXT NOT NULL DEFAULT '[]',
  caps_json TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  UNIQUE(project_id, name)
);
CREATE INDEX IF NOT EXISTS idx_environments_project ON environments(project_id);

CREATE TABLE IF NOT EXISTS secret_refs (
  id TEXT PRIMARY KEY,
  environment_id TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(environment_id, key)
);

CREATE TABLE IF NOT EXISTS scenarios (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  revision INTEGER NOT NULL DEFAULT 1,
  definition_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  UNIQUE(project_id, name)
);

CREATE TABLE IF NOT EXISTS load_profiles (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 1,
  definition_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  UNIQUE(project_id, name)
);

CREATE TABLE IF NOT EXISTS experiments (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  scenario_id TEXT NOT NULL REFERENCES scenarios(id),
  profile_id TEXT NOT NULL REFERENCES load_profiles(id),
  environment_id TEXT NOT NULL REFERENCES environments(id),
  objectives_json TEXT NOT NULL DEFAULT '[]',
  baseline_run_id TEXT,
  notes TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  UNIQUE(project_id, name)
);
CREATE INDEX IF NOT EXISTS idx_experiments_project ON experiments(project_id);

CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  experiment_id TEXT NOT NULL REFERENCES experiments(id) ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN (
    'preflight','starting','running','stopping','finalizing',
    'completed','failed','aborted','interrupted'
  )),
  verdict TEXT CHECK (verdict IN ('passed','failed','inconclusive','aborted','error')),
  trigger TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  engine_id TEXT NOT NULL,
  engine_version TEXT NOT NULL,
  config_hash TEXT NOT NULL,
  snapshot_json TEXT NOT NULL,
  summary_json TEXT,
  objectives_json TEXT NOT NULL DEFAULT '[]',
  error_message TEXT,
  artifact_dir TEXT NOT NULL,
  host_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_runs_project_started ON runs(project_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_experiment_started ON runs(experiment_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status);

CREATE TABLE IF NOT EXISTS run_buckets (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  bucket_ix INTEGER NOT NULL,
  elapsed_ms INTEGER NOT NULL,
  requests INTEGER NOT NULL DEFAULT 0,
  failed INTEGER NOT NULL DEFAULT 0,
  http_errors INTEGER NOT NULL DEFAULT 0,
  assertion_failures INTEGER NOT NULL DEFAULT 0,
  timeouts INTEGER NOT NULL DEFAULT 0,
  conn_failures INTEGER NOT NULL DEFAULT 0,
  iterations INTEGER NOT NULL DEFAULT 0,
  p50_ms REAL,
  p90_ms REAL,
  p95_ms REAL,
  p99_ms REAL,
  mean_ms REAL,
  min_ms REAL,
  max_ms REAL,
  bytes_in INTEGER NOT NULL DEFAULT 0,
  bytes_out INTEGER NOT NULL DEFAULT 0,
  vus INTEGER,
  PRIMARY KEY (run_id, bucket_ix)
);

CREATE TABLE IF NOT EXISTS run_steps (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  step_id TEXT NOT NULL,
  step_name TEXT NOT NULL,
  requests INTEGER NOT NULL DEFAULT 0,
  failed INTEGER NOT NULL DEFAULT 0,
  assertion_failures INTEGER NOT NULL DEFAULT 0,
  timeouts INTEGER NOT NULL DEFAULT 0,
  conn_failures INTEGER NOT NULL DEFAULT 0,
  p50_ms REAL,
  p95_ms REAL,
  p99_ms REAL,
  mean_ms REAL,
  min_ms REAL,
  max_ms REAL,
  PRIMARY KEY (run_id, step_id)
);

CREATE TABLE IF NOT EXISTS run_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  elapsed_ms INTEGER NOT NULL,
  level TEXT NOT NULL,
  kind TEXT NOT NULL,
  message TEXT NOT NULL,
  data_json TEXT,
  UNIQUE(run_id, seq)
);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);

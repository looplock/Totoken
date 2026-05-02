CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    source_app TEXT NOT NULL,
    external_session_id TEXT,
    session_key TEXT UNIQUE NOT NULL,
    title TEXT,
    model_first TEXT,
    model_last TEXT,
    source_created_at DATETIME,
    source_updated_at DATETIME,
    discovered_first_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    discovered_last_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    source_state TEXT DEFAULT 'synced'
);

CREATE TABLE IF NOT EXISTS session_token_totals (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    input_tokens_max INTEGER DEFAULT 0,
    output_tokens_max INTEGER DEFAULT 0,
    total_tokens_max INTEGER DEFAULT 0,
    last_observed_at DATETIME,
    last_observation_id TEXT
);

CREATE TABLE IF NOT EXISTS session_observations (
    id TEXT PRIMARY KEY,
    session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    observed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    input_tokens INTEGER,
    output_tokens INTEGER,
    total_tokens INTEGER,
    conversation_checksum TEXT,
    message_count INTEGER,
    source_model TEXT,
    scan_run_id TEXT,
    UNIQUE(session_id, conversation_checksum)
);

CREATE TABLE IF NOT EXISTS session_requests (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    observation_id TEXT NOT NULL REFERENCES session_observations(id) ON DELETE CASCADE,
    source_app TEXT NOT NULL,
    source_request_id TEXT,
    sequence_no INTEGER NOT NULL,
    status TEXT,
    message_count INTEGER NOT NULL,
    model TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    total_tokens INTEGER,
    cache_read_input_tokens INTEGER,
    cache_write_input_tokens INTEGER,
    estimated_cost_usd REAL,
    token_confidence TEXT,
    source_created_at DATETIME,
    source_updated_at DATETIME,
    source_locator TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(observation_id, sequence_no),
    UNIQUE(source_app, source_request_id)
);

CREATE TABLE IF NOT EXISTS token_usage_events (
    id TEXT PRIMARY KEY,
    session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    observation_id TEXT REFERENCES session_observations(id) ON DELETE CASCADE,
    event_time_utc DATETIME NOT NULL,
    event_timezone TEXT,
    delta_input INTEGER DEFAULT 0,
    delta_output INTEGER DEFAULT 0,
    delta_total INTEGER DEFAULT 0,
    cache_read_input_tokens INTEGER DEFAULT 0,
    cache_write_input_tokens INTEGER DEFAULT 0,
    estimated_cost_usd REAL,
    source_app TEXT NOT NULL,
    model TEXT,
    granularity TEXT,
    confidence TEXT,
    source_event_id TEXT,
    epoch_no INTEGER DEFAULT 0,
    UNIQUE(source_app, source_event_id)
);

CREATE TABLE IF NOT EXISTS source_configs (
    id TEXT PRIMARY KEY,
    source_type TEXT NOT NULL,
    root_path TEXT NOT NULL,
    enabled INTEGER DEFAULT 1,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS session_source_refs (
    session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    source_path TEXT NOT NULL,
    source_file_id TEXT,
    last_linked_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (session_id, source_path)
);

CREATE TABLE IF NOT EXISTS source_files_cache (
    id TEXT PRIMARY KEY,
    source_config_id TEXT REFERENCES source_configs(id),
    abs_path TEXT UNIQUE NOT NULL,
    size_bytes INTEGER,
    mtime_ms INTEGER,
    fingerprint_fast TEXT,
    fingerprint_strong TEXT,
    parser_version INTEGER DEFAULT 1,
    last_scan_at DATETIME,
    last_parse_status TEXT,
    last_error TEXT
);

CREATE TABLE IF NOT EXISTS scan_runs (
    id TEXT PRIMARY KEY,
    trigger_type TEXT,
    started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    ended_at DATETIME,
    status TEXT,
    files_seen INTEGER DEFAULT 0,
    files_parsed INTEGER DEFAULT 0,
    files_skipped INTEGER DEFAULT 0,
    files_failed INTEGER DEFAULT 0,
    sessions_changed INTEGER DEFAULT 0,
    error_count INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS model_catalog (
    id TEXT PRIMARY KEY,
    canonical_key TEXT UNIQUE NOT NULL,
    provider TEXT NOT NULL,
    api_family TEXT NOT NULL,
    model_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    description TEXT,
    context_window INTEGER,
    max_output_tokens INTEGER,
    input_modalities_json TEXT NOT NULL DEFAULT '[]',
    output_modalities_json TEXT NOT NULL DEFAULT '[]',
    capabilities_json TEXT NOT NULL DEFAULT '[]',
    supported_parameters_json TEXT NOT NULL DEFAULT '[]',
    pricing_input_usd_per_mtok REAL,
    pricing_output_usd_per_mtok REAL,
    pricing_cache_read_usd_per_mtok REAL,
    pricing_cache_write_usd_per_mtok REAL,
    docs_url TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    raw_source TEXT NOT NULL,
    source_payload_json TEXT,
    last_synced_at DATETIME,
    last_verified_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS model_aliases (
    alias TEXT PRIMARY KEY,
    model_catalog_id TEXT NOT NULL REFERENCES model_catalog(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    source TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS model_sync_runs (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    ended_at DATETIME,
    status TEXT NOT NULL,
    models_seen INTEGER DEFAULT 0,
    models_inserted INTEGER DEFAULT 0,
    models_updated INTEGER DEFAULT 0,
    error_count INTEGER DEFAULT 0,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_session_requests_session_observation
    ON session_requests(session_id, observation_id, sequence_no);

CREATE INDEX IF NOT EXISTS idx_session_requests_created_at
    ON session_requests(source_created_at DESC, source_updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_session_observations_session_id
    ON session_observations(session_id);

CREATE INDEX IF NOT EXISTS idx_token_usage_events_session_id
    ON token_usage_events(session_id);

CREATE INDEX IF NOT EXISTS idx_token_usage_events_event_time_utc
    ON token_usage_events(event_time_utc DESC);

CREATE INDEX IF NOT EXISTS idx_model_catalog_provider
    ON model_catalog(provider);

CREATE INDEX IF NOT EXISTS idx_model_catalog_status
    ON model_catalog(status);

CREATE INDEX IF NOT EXISTS idx_model_catalog_model_id
    ON model_catalog(model_id);

CREATE INDEX IF NOT EXISTS idx_model_sync_runs_started_at
    ON model_sync_runs(started_at DESC);

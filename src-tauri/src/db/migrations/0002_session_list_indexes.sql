CREATE INDEX IF NOT EXISTS idx_sessions_source_app_state_discovered
    ON sessions(source_app, source_state, discovered_last_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_sessions_discovered_last
    ON sessions(discovered_last_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_sessions_source_updated
    ON sessions(source_updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_sessions_source_app
    ON sessions(source_app);

CREATE INDEX IF NOT EXISTS idx_sessions_source_state
    ON sessions(source_state);

CREATE INDEX IF NOT EXISTS idx_session_requests_low_confidence_session
    ON session_requests(token_confidence, session_id);

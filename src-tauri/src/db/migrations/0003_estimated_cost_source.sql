ALTER TABLE session_requests
ADD COLUMN estimated_cost_source TEXT;

ALTER TABLE token_usage_events
ADD COLUMN estimated_cost_source TEXT;

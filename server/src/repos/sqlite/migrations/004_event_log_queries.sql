CREATE INDEX event_logs_by_account_all ON event_logs (account_id, seq DESC);
CREATE INDEX event_logs_by_account_all_type ON event_logs (account_id, event_type, seq DESC);
CREATE INDEX event_logs_by_correlation ON event_logs (json_extract(payload_json, '$.correlation_id'), seq ASC);

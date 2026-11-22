CREATE INDEX event_logs_by_type ON event_logs (event_type, seq DESC);
CREATE INDEX event_logs_by_source_type ON event_logs (source_id, event_type, seq DESC);
CREATE INDEX event_logs_by_account_type ON event_logs (account_id, event_type, seq DESC)
    WHERE source_id IS NULL;

CREATE TABLE event_log_counts (
    event_type TEXT PRIMARY KEY NOT NULL,
    row_count  INTEGER NOT NULL CHECK (row_count >= 0)
);

INSERT INTO event_log_counts (event_type, row_count)
SELECT event_type, count(*) FROM event_logs GROUP BY event_type;

CREATE TRIGGER event_logs_count_insert AFTER INSERT ON event_logs BEGIN
    INSERT INTO event_log_counts (event_type, row_count) VALUES (NEW.event_type, 1)
    ON CONFLICT(event_type) DO UPDATE SET row_count = row_count + 1;
END;

CREATE TRIGGER event_logs_count_delete AFTER DELETE ON event_logs BEGIN
    UPDATE event_log_counts SET row_count = row_count - 1 WHERE event_type = OLD.event_type;
END;

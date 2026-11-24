CREATE TABLE accounts (
    account_id       TEXT PRIMARY KEY NOT NULL,
    provider         TEXT NOT NULL,
    upstream_userid  TEXT NOT NULL,
    status           TEXT NOT NULL,
    cookies_json     TEXT NOT NULL,
    last_refresh_at  INTEGER,
    credential_version INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT,
    last_error_code  TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    UNIQUE (provider, upstream_userid)
);

CREATE TABLE account_entitlements (
    account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    items_json TEXT,
    synced_at INTEGER,
    sync_failed INTEGER NOT NULL CHECK (sync_failed IN (0, 1)),
    svip_evidence_json TEXT
);

CREATE TABLE sources (
    source_id        TEXT PRIMARY KEY NOT NULL,
    account_id       TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    provider         TEXT NOT NULL,
    name             TEXT NOT NULL,
    script_token     TEXT NOT NULL UNIQUE,
    runtime_token    TEXT NOT NULL UNIQUE,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    quality_fallback INTEGER NOT NULL CHECK (quality_fallback IN (0, 1))
);
CREATE INDEX sources_by_account ON sources (account_id, created_at, source_id);

CREATE TABLE login_sessions (
    session_id   TEXT PRIMARY KEY NOT NULL,
    session_json TEXT NOT NULL,
    expires_at   INTEGER NOT NULL
);

CREATE TABLE event_logs (
    seq          INTEGER PRIMARY KEY,
    source_id    TEXT,
    account_id   TEXT,
    event_type   TEXT NOT NULL,
    ok           INTEGER NOT NULL CHECK (ok IN (0, 1)),
    correlation_id TEXT NOT NULL,
    payload_json TEXT NOT NULL
);
CREATE INDEX event_logs_by_source ON event_logs (source_id, seq DESC);
CREATE INDEX login_sessions_by_expiry ON login_sessions (expires_at);

CREATE INDEX event_logs_by_type ON event_logs (event_type, seq DESC);
CREATE INDEX event_logs_by_source_type ON event_logs (source_id, event_type, seq DESC);

CREATE TABLE event_log_counts (
    event_type TEXT PRIMARY KEY NOT NULL,
    row_count  INTEGER NOT NULL CHECK (row_count >= 0)
);

CREATE TRIGGER event_logs_count_insert AFTER INSERT ON event_logs BEGIN
    INSERT INTO event_log_counts (event_type, row_count) VALUES (NEW.event_type, 1)
    ON CONFLICT(event_type) DO UPDATE SET row_count = row_count + 1;
END;

CREATE TRIGGER event_logs_count_delete AFTER DELETE ON event_logs BEGIN
    UPDATE event_log_counts SET row_count = row_count - 1 WHERE event_type = OLD.event_type;
END;

CREATE TABLE entitlement_renewal (
    account_id         TEXT PRIMARY KEY NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    auto_claim_enabled INTEGER NOT NULL DEFAULT 0 CHECK (auto_claim_enabled IN (0, 1)),
    rate_limit_until   INTEGER,
    rate_limit_streak  INTEGER NOT NULL DEFAULT 0,
    propagating_until  INTEGER
);

CREATE TABLE entitlement_daily_runs (
    account_id    TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    upstream_day  TEXT NOT NULL,
    progress_json TEXT NOT NULL,
    unsettled     INTEGER GENERATED ALWAYS AS (
        json_extract(progress_json, '$.recovery_closed') = 0
        AND (json_extract(progress_json, '$.claim.state') = 'uncertain'
             OR json_extract(progress_json, '$.upgrade.state') = 'uncertain')
    ),
    PRIMARY KEY (account_id, upstream_day)
);

CREATE INDEX unsettled_entitlement_days ON entitlement_daily_runs (account_id, upstream_day)
WHERE unsettled;

CREATE INDEX event_logs_by_account_all ON event_logs (account_id, seq DESC);
CREATE INDEX event_logs_by_account_all_type ON event_logs (account_id, event_type, seq DESC);
CREATE INDEX event_logs_by_correlation ON event_logs (correlation_id, seq ASC);

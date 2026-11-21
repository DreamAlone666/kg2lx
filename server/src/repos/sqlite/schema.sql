CREATE TABLE accounts (
    account_id       TEXT PRIMARY KEY NOT NULL,
    provider         TEXT NOT NULL,
    upstream_userid  TEXT NOT NULL,
    display_name     TEXT,
    status           TEXT NOT NULL,
    cookies_json     TEXT NOT NULL,
    last_refresh_at  INTEGER,
    last_success_at  INTEGER,
    last_error       TEXT,
    last_error_code  TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    UNIQUE (provider, upstream_userid)
);

CREATE TABLE account_entitlements (
    account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(account_id),
    items_json TEXT NOT NULL
);

CREATE TABLE sources (
    source_id        TEXT PRIMARY KEY NOT NULL,
    account_id       TEXT NOT NULL REFERENCES accounts(account_id),
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
    session_json TEXT NOT NULL
);

CREATE TABLE event_logs (
    seq          INTEGER PRIMARY KEY,
    source_id    TEXT NOT NULL,
    ok           INTEGER NOT NULL,
    payload_json TEXT NOT NULL
);
CREATE INDEX event_logs_by_source ON event_logs (source_id, seq DESC);

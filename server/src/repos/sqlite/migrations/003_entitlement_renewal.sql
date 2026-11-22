ALTER TABLE account_entitlements ADD COLUMN svip_evidence_json TEXT;

CREATE TABLE entitlement_renewal (
    account_id         TEXT PRIMARY KEY NOT NULL REFERENCES accounts(account_id),
    auto_claim_enabled INTEGER NOT NULL DEFAULT 0 CHECK (auto_claim_enabled IN (0, 1)),
    rate_limit_until   INTEGER,
    rate_limit_streak  INTEGER NOT NULL DEFAULT 0,
    propagating_until  INTEGER
);

CREATE TABLE entitlement_daily_runs (
    account_id    TEXT NOT NULL REFERENCES accounts(account_id),
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

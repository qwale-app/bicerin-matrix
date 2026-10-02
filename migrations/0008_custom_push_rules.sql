-- User-managed Matrix push rules. Default-rule enablement remains in
-- push_rule_overrides so deployments upgrade without rewriting existing data.
CREATE TABLE IF NOT EXISTS push_rules (
    user_id     TEXT NOT NULL,
    kind        TEXT NOT NULL,
    rule_id     TEXT NOT NULL,
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    conditions  JSONB NOT NULL DEFAULT '[]',
    actions     JSONB NOT NULL DEFAULT '[]',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, kind, rule_id)
);
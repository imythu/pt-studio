-- ratio-policy（docs/plugins/ratio-policy/03 §3）。偏差：history 不分区（每日 cron 清理 2 年前的记录）。
CREATE TABLE states (
    user_id          bigint PRIMARY KEY,
    state            text NOT NULL DEFAULT 'ok' CHECK (state IN ('ok', 'warned', 'restricted', 'banned', 'restricted_skipped')),
    manual           boolean NOT NULL DEFAULT false,
    warned_at        timestamptz,
    deadline         timestamptz,
    restricted_at    timestamptz,
    restricted_seq   integer NOT NULL DEFAULT 0,
    exempt_until     timestamptz,
    exempt_reason    text,
    last_ratio       double precision,
    last_required    double precision,
    evaluated_at     timestamptz,
    updated_at       timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX states_active_idx ON states (state) WHERE state <> 'ok';
CREATE INDEX states_deadline_idx ON states (deadline) WHERE state = 'warned';

CREATE TABLE dirty_users (
    user_id  bigint PRIMARY KEY,
    since    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE history (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at          timestamptz NOT NULL DEFAULT now(),
    user_id     bigint NOT NULL,
    from_state  text NOT NULL,
    to_state    text NOT NULL,
    ratio       double precision,
    required    double precision,
    actor       text NOT NULL,
    reason      text NOT NULL DEFAULT ''
);
CREATE INDEX history_user_idx ON history (user_id, at DESC);

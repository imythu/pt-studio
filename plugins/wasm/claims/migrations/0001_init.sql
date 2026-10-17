-- claims（docs/plugins/claims/README.md）：种子认领与月度结算
CREATE TABLE claims (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id         bigint NOT NULL,
    torrent_id      bigint NOT NULL,
    state           text NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'removed')),
    claimed_at      timestamptz NOT NULL DEFAULT now(),
    removed_at      timestamptz,
    remove_reason   text,
    -- 当前考核周期的开始时间与该时刻的累计做种秒数
    period_start    timestamptz NOT NULL DEFAULT now(),
    seed_secs_base  bigint NOT NULL DEFAULT 0,
    fails           integer NOT NULL DEFAULT 0,
    last_result     text CHECK (last_result IN ('passed', 'failed'))
);
CREATE UNIQUE INDEX claims_active_key ON claims (user_id, torrent_id) WHERE state = 'active';
CREATE INDEX claims_torrent_idx ON claims (torrent_id) WHERE state = 'active';
CREATE INDEX claims_user_idx ON claims (user_id, claimed_at DESC);

CREATE TABLE settlements (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    period      date NOT NULL,
    user_id     bigint NOT NULL,
    claims      integer NOT NULL,
    passed      integer NOT NULL,
    failed      integer NOT NULL,
    removed     integer NOT NULL DEFAULT 0,
    reward      bigint NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (period, user_id)
);

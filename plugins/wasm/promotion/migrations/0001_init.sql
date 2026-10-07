-- promotion 0001（docs/plugins/promotion/03 §3.1）。search_path = promotion。
CREATE TABLE promotions (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source          text NOT NULL CHECK (source IN ('manual', 'auto', 'global', 'card')),
    torrent_id      bigint,
    user_id         bigint,
    rule_id         bigint,
    type_ids        text[],
    kind            text NOT NULL,
    up_permille     integer,
    down_permille   integer,
    priority        integer NOT NULL DEFAULT 0,
    starts_at       timestamptz NOT NULL,
    ends_at         timestamptz,
    reason          text NOT NULL DEFAULT '',
    created_by      text NOT NULL,                 -- 'user:<id>' / 'plugin:<id>' / 'auto:<rule_id>'
    state           text NOT NULL CHECK (state IN ('active', 'ended')),
    end_cause       text,
    ended_at        timestamptz,
    external_ref    text UNIQUE,
    rate_rule_id    bigint,
    sync_state      text NOT NULL CHECK (sync_state IN ('pending', 'synced', 'remove_pending', 'removed', 'not_applicable')),
    sync_error      text,
    sync_attempts   integer NOT NULL DEFAULT 0,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT promotions_target CHECK (
        (source = 'global' AND torrent_id IS NULL AND user_id IS NULL) OR
        (source IN ('manual', 'auto') AND torrent_id IS NOT NULL AND user_id IS NULL) OR
        (source = 'card' AND torrent_id IS NOT NULL AND user_id IS NOT NULL))
);
CREATE UNIQUE INDEX promotions_one_manual_idx ON promotions (torrent_id) WHERE source = 'manual' AND state = 'active';
CREATE UNIQUE INDEX promotions_one_auto_idx ON promotions (torrent_id) WHERE source = 'auto' AND state = 'active';
CREATE UNIQUE INDEX promotions_one_card_idx ON promotions (user_id, torrent_id, kind) WHERE source = 'card' AND state = 'active';
CREATE INDEX promotions_torrent_active_idx ON promotions (torrent_id) WHERE state = 'active';
CREATE INDEX promotions_expiry_idx ON promotions (ends_at) WHERE state = 'active' AND ends_at IS NOT NULL;
CREATE INDEX promotions_sync_idx ON promotions (sync_state) WHERE sync_state IN ('pending', 'remove_pending');
CREATE INDEX promotions_global_idx ON promotions (starts_at, ends_at) WHERE source = 'global' AND state = 'active';

CREATE TABLE auto_rules (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    definition  jsonb NOT NULL,
    priority    integer NOT NULL,
    active      boolean NOT NULL DEFAULT true,
    created_by  bigint,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE auto_state (
    rule_id     bigint NOT NULL REFERENCES auto_rules (id) ON DELETE CASCADE,
    torrent_id  bigint NOT NULL,
    fired_once  boolean NOT NULL DEFAULT false,
    entered_at  timestamptz,
    PRIMARY KEY (rule_id, torrent_id)
);

CREATE TABLE torrent_mirror (
    torrent_id  bigint PRIMARY KEY,
    type_id     text,
    size        bigint,
    status      text,
    tags        text[] NOT NULL DEFAULT '{}',
    file_count  integer,
    created_at  timestamptz,
    seeders     integer NOT NULL DEFAULT 0,
    leechers    integer NOT NULL DEFAULT 0,
    completed   integer NOT NULL DEFAULT 0,
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX torrent_mirror_created_idx ON torrent_mirror (created_at) WHERE status = 'approved';
CREATE INDEX torrent_mirror_seeders_idx ON torrent_mirror (seeders) WHERE status = 'approved';

CREATE TABLE card_balances (
    user_id    bigint NOT NULL,
    card_kind  text NOT NULL CHECK (card_kind IN ('free', 'double_up')),
    balance    integer NOT NULL DEFAULT 0 CHECK (balance >= 0),
    PRIMARY KEY (user_id, card_kind)
);

CREATE TABLE card_log (
    id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id        bigint NOT NULL,
    card_kind      text NOT NULL,
    delta          integer NOT NULL,
    balance_after  integer NOT NULL,
    reason         text NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX card_log_user_idx ON card_log (user_id, id DESC);

CREATE TABLE history (
    id            bigint GENERATED ALWAYS AS IDENTITY,
    at            timestamptz NOT NULL DEFAULT now(),
    promotion_id  bigint NOT NULL,
    torrent_id    bigint,
    action        text NOT NULL CHECK (action IN ('start', 'end', 'replace', 'extend')),
    kind          text NOT NULL,
    source        text NOT NULL,
    actor         text NOT NULL,
    reason        text NOT NULL DEFAULT '',
    PRIMARY KEY (at, id)
) PARTITION BY RANGE (at);
CREATE INDEX history_torrent_idx ON history (torrent_id, at DESC);

CREATE TABLE idempotency (
    caller text NOT NULL, key text NOT NULL, request_hash bytea NOT NULL,
    response jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (caller, key)
);

-- 全站活动的标题（terms 插件接入前存于本表）
ALTER TABLE promotions ADD COLUMN title jsonb;

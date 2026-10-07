-- levels 0001（docs/plugins/levels/03 §3.1）。search_path = levels。
CREATE TABLE classes (
    id              integer PRIMARY KEY CHECK (id BETWEEN 1 AND 65535),
    slug            text NOT NULL UNIQUE,
    rank            integer NOT NULL,
    type            text NOT NULL CHECK (type IN ('normal', 'vip', 'honor')),
    promote         jsonb NOT NULL DEFAULT '[]',
    keep            jsonb,
    roles           text[] NOT NULL DEFAULT '{}',
    can_invite      boolean NOT NULL DEFAULT true,
    reward_invites  integer NOT NULL DEFAULT 0 CHECK (reward_invites BETWEEN 0 AND 100),
    protected       boolean NOT NULL DEFAULT false,
    icon            text,
    color           text,
    active          boolean NOT NULL DEFAULT true,
    preset          text,
    is_default      boolean NOT NULL DEFAULT false,      -- 新用户的初始等级（实现补充）
    revision        integer NOT NULL DEFAULT 1,
    updated_at      timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX classes_normal_rank_idx ON classes (rank) WHERE type = 'normal' AND active;

CREATE TABLE user_levels (
    user_id            bigint PRIMARY KEY,
    class_id           integer NOT NULL,
    base_class_id      integer NOT NULL,
    source             text NOT NULL CHECK (source IN ('auto', 'manual', 'vip', 'honor', 'default')),
    protect_until      timestamptz,             -- 升级保护期
    protected_until    timestamptz,             -- 管理员保号
    warning_class_id   integer,
    warning_deadline   timestamptz,
    warning_failed     jsonb,
    last_active_day    date,
    last_inactive_notice date,
    evaluated_at       timestamptz,
    updated_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX user_levels_warning_idx ON user_levels (warning_deadline) WHERE warning_deadline IS NOT NULL;
CREATE INDEX user_levels_active_idx ON user_levels (last_active_day);

CREATE TABLE overrides (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id     bigint NOT NULL,
    kind        text NOT NULL CHECK (kind IN ('manual', 'honor', 'vip')),
    class_id    integer NOT NULL REFERENCES classes (id),
    until       timestamptz,
    reason      text NOT NULL DEFAULT '',
    created_by  text NOT NULL,
    source_ref  text UNIQUE,
    ended_at    timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX overrides_user_idx ON overrides (user_id) WHERE ended_at IS NULL;
CREATE INDEX overrides_until_idx ON overrides (until) WHERE ended_at IS NULL AND until IS NOT NULL;

CREATE TABLE dirty_users (
    user_id  bigint PRIMARY KEY,
    since    timestamptz NOT NULL
);

CREATE TABLE rewards_given (
    user_id   bigint NOT NULL,
    class_id  integer NOT NULL,
    state     text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'done', 'skipped')),
    PRIMARY KEY (user_id, class_id)
);

CREATE TABLE history (
    id          bigint GENERATED ALWAYS AS IDENTITY,
    at          timestamptz NOT NULL DEFAULT now(),
    user_id     bigint NOT NULL,
    from_class  integer,
    to_class    integer NOT NULL,
    cause       text NOT NULL,
    actor       text NOT NULL,
    detail      jsonb,
    PRIMARY KEY (at, id)
) PARTITION BY RANGE (at);
CREATE INDEX history_user_idx ON history (user_id, at DESC);

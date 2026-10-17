-- newbie-exam（docs/plugins/newbie-exam/03 §3 子集）。偏差：考核定义在插件设置中（单一考核），分配时快照到 assignments。
CREATE TABLE assignments (
    id                   bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id              bigint NOT NULL,
    state                text NOT NULL CHECK (state IN ('active', 'passed', 'failed', 'failed_restricted', 'failed_banned', 'remedied', 'exempted')),
    assigned_at          timestamptz NOT NULL DEFAULT now(),
    baseline             jsonb NOT NULL DEFAULT '{}',
    definition_snapshot  jsonb NOT NULL,
    stage_index          smallint NOT NULL DEFAULT 0,
    stage_deadline       timestamptz,
    prewarned            boolean NOT NULL DEFAULT false,
    result_at            timestamptz,
    reward_given         boolean NOT NULL DEFAULT false,
    reason               text,
    assigned_by          text NOT NULL,
    updated_at           timestamptz NOT NULL DEFAULT now()
);
-- 每人同时只有一个进行中 / 失败限制中的考核
CREATE UNIQUE INDEX assignments_open_key ON assignments (user_id) WHERE state IN ('active', 'failed_restricted');
CREATE INDEX assignments_deadline_idx ON assignments (stage_deadline) WHERE state = 'active';
CREATE INDEX assignments_user_idx ON assignments (user_id, assigned_at DESC);

CREATE TABLE dirty_users (
    user_id  bigint PRIMARY KEY,
    since    timestamptz NOT NULL DEFAULT now()
);

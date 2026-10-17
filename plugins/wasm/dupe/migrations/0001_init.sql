-- 疑似重复（同作品同规格槽位）：审核队列
CREATE TABLE candidates (
    id          bigserial PRIMARY KEY,
    torrent_id  bigint NOT NULL,
    matches     bigint[] NOT NULL,
    slot        text NOT NULL,
    state       text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'dismissed', 'trumped')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    resolved_by bigint,
    resolved_at timestamptz
);
CREATE UNIQUE INDEX candidates_torrent_idx ON candidates (torrent_id);
CREATE INDEX candidates_open_idx ON candidates (created_at) WHERE state = 'open';

-- 替代：新种子替代旧种子（旧种子下架，通知完成过旧种子的用户）
CREATE TABLE trumps (
    id           bigserial PRIMARY KEY,
    old_id       bigint NOT NULL,
    new_id       bigint NOT NULL CHECK (new_id <> old_id),
    reason       text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    state        text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'approved', 'rejected')),
    requested_by bigint NOT NULL,
    decided_by   bigint,
    decide_note  text,
    notified     integer NOT NULL DEFAULT 0,
    created_at   timestamptz NOT NULL DEFAULT now(),
    decided_at   timestamptz
);
CREATE UNIQUE INDEX trumps_open_old_idx ON trumps (old_id) WHERE state IN ('pending', 'approved');
CREATE INDEX trumps_new_idx ON trumps (new_id);
CREATE INDEX trumps_pending_idx ON trumps (created_at) WHERE state = 'pending';

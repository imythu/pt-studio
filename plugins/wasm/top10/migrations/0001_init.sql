-- 排行榜快照：每小时由 refresh 任务整体重算（board × period 的前 N 名）
CREATE TABLE entries (
    board     text NOT NULL,
    period    text NOT NULL CHECK (period IN ('day', 'week', 'month', 'all', 'now')),
    rank      integer NOT NULL,
    entity_id bigint NOT NULL,
    value     bigint NOT NULL,
    PRIMARY KEY (board, period, rank)
);

CREATE TABLE refreshes (
    id           boolean PRIMARY KEY DEFAULT true CHECK (id),
    refreshed_at timestamptz NOT NULL
);

-- 不出现在用户榜单中的用户（自己选择退出）
CREATE TABLE opt_out (
    user_id    bigint PRIMARY KEY,
    created_at timestamptz NOT NULL DEFAULT now()
);

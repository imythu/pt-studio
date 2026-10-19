-- 抽奖改为幸运转盘：移除期次制（rounds / tickets / prizes），改为奖品表 + 抽奖记录
DROP TABLE prizes;
DROP TABLE tickets;
DROP TABLE rounds;

-- 奖品：kind 决定 amount 的含义（bonus：魔力值；upload：字节；vip_days：天数；vip_permanent：忽略）
-- chance_ppm：中奖概率（百万分之一）；启用的奖品概率之和 ≤ 1 000 000，剩余部分为“谢谢参与”
-- stock：剩余数量，NULL 表示不限；抽完后不再出现在转盘上
CREATE TABLE wheel_prizes (
    id          bigserial PRIMARY KEY,
    kind        text NOT NULL CHECK (kind IN ('bonus', 'upload', 'vip_days', 'vip_permanent')),
    amount      bigint NOT NULL DEFAULT 0 CHECK (amount >= 0),
    label       text NOT NULL DEFAULT '' CHECK (char_length(label) <= 40),
    chance_ppm  integer NOT NULL CHECK (chance_ppm BETWEEN 0 AND 1000000),
    stock       integer CHECK (stock >= 0),
    sort        integer NOT NULL DEFAULT 100,
    active      boolean NOT NULL DEFAULT true,
    deleted_at  timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- 抽奖记录：prize_id 为空表示未中奖；kind / amount 为当时奖品的快照
CREATE TABLE spins (
    id          bigserial PRIMARY KEY,
    user_id     bigint NOT NULL,
    cost        bigint NOT NULL DEFAULT 0,
    prize_id    bigint REFERENCES wheel_prizes (id),
    kind        text,
    amount      bigint NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX spins_user_idx ON spins (user_id, id DESC);
CREATE INDEX spins_won_idx ON spins (id DESC) WHERE prize_id IS NOT NULL;

-- 每用户一行，抽奖时行锁串行化同一用户的并发请求（每日次数检查与写入一致）
CREATE TABLE spin_locks (
    user_id bigint PRIMARY KEY
);

-- 默认奖品（管理员可在后台调整概率与库存）
INSERT INTO wheel_prizes (kind, amount, chance_ppm, stock, sort) VALUES
    ('bonus', 100, 200000, NULL, 10),
    ('bonus', 500, 50000, NULL, 20),
    ('upload', 10737418240, 100000, NULL, 30),
    ('upload', 53687091200, 20000, NULL, 40),
    ('vip_days', 7, 10000, NULL, 50),
    ('vip_days', 30, 2000, NULL, 60),
    ('vip_permanent', 0, 100, 1, 70);

-- 物品：勋章（佩戴展示）或头衔（同时只能激活一个）
CREATE TABLE items (
    id            bigserial PRIMARY KEY,
    kind          text NOT NULL CHECK (kind IN ('medal', 'title')),
    name          text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 40),
    description   text NOT NULL DEFAULT '' CHECK (char_length(description) <= 300),
    icon_key      text,
    max_holders   integer CHECK (max_holders > 0),
    validity_days integer CHECK (validity_days BETWEEN 1 AND 3650),
    hidden        boolean NOT NULL DEFAULT false,
    active        boolean NOT NULL DEFAULT true,
    sort          integer NOT NULL DEFAULT 0,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

-- 授予：source_ref 确定性、唯一（成就 ach:<rule>:<uid>、shop shop:fulfill:<order>、管理员 admin:<grant>:<uid>）
CREATE TABLE grants (
    id            bigserial PRIMARY KEY,
    user_id       bigint NOT NULL,
    item_id       bigint NOT NULL REFERENCES items (id),
    source        text NOT NULL CHECK (source IN ('admin', 'achievement', 'plugin')),
    source_plugin text,
    source_ref    text NOT NULL UNIQUE CHECK (char_length(source_ref) <= 200),
    reason        text NOT NULL DEFAULT '' CHECK (char_length(reason) <= 300),
    granted_by    bigint,
    granted_at    timestamptz NOT NULL DEFAULT now(),
    expires_at    timestamptz,
    revoked_at    timestamptz,
    revoke_reason text
);
CREATE INDEX grants_user_idx ON grants (user_id) WHERE revoked_at IS NULL;
CREATE INDEX grants_item_idx ON grants (item_id) WHERE revoked_at IS NULL;

-- 展示设置：佩戴的勋章（有序物品 ID）与激活的头衔
CREATE TABLE display (
    user_id    bigint PRIMARY KEY,
    worn       bigint[] NOT NULL DEFAULT '{}',
    title_item bigint REFERENCES items (id),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- 成就规则：event = 事件计数 / 取最大值 / 求和，stat = 每日按累计数据评估
CREATE TABLE rules (
    id          bigserial PRIMARY KEY,
    item_id     bigint NOT NULL REFERENCES items (id),
    kind        text NOT NULL CHECK (kind IN ('event', 'stat')),
    event       text,
    measure     text NOT NULL DEFAULT 'count',
    stat_field  text,
    threshold   bigint NOT NULL CHECK (threshold > 0),
    description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 300),
    active      boolean NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now(),
    CHECK ((kind = 'event' AND event IS NOT NULL) OR (kind = 'stat' AND stat_field IS NOT NULL))
);

CREATE TABLE counters (
    rule_id bigint NOT NULL REFERENCES rules (id) ON DELETE CASCADE,
    user_id bigint NOT NULL,
    value   bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (rule_id, user_id)
);

-- 服务调用幂等（撤销）
CREATE TABLE idempotency (
    caller     text NOT NULL,
    key        text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (caller, key)
);

-- 预置：几枚常见成就勋章（默认规则启用）
INSERT INTO items (kind, name, description, sort) VALUES
  ('medal', '初次发布', '发布第一个种子', 10),
  ('medal', '发种达人', '发布 100 个种子', 20),
  ('medal', '签到之星', '连续签到 30 天', 30),
  ('medal', '字幕组', '上传 10 个字幕', 40),
  ('medal', '1 TiB 上传', '累计上传量达到 1 TiB', 50);
INSERT INTO rules (item_id, kind, event, measure, stat_field, threshold, description)
SELECT id, 'event', 'torrent.created', 'count', NULL, 1, '发布 1 个种子' FROM items WHERE name = '初次发布'
UNION ALL SELECT id, 'event', 'torrent.created', 'count', NULL, 100, '发布 100 个种子' FROM items WHERE name = '发种达人'
UNION ALL SELECT id, 'event', 'checkin.checked_in', 'max:streak', NULL, 30, '连续签到 30 天' FROM items WHERE name = '签到之星'
UNION ALL SELECT id, 'event', 'subtitle.uploaded', 'count', NULL, 10, '上传 10 个字幕' FROM items WHERE name = '字幕组'
UNION ALL SELECT id, 'stat', NULL, 'count', 'uploaded', 1099511627776, '累计上传 1 TiB' FROM items WHERE name = '1 TiB 上传';

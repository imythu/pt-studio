-- levels 0002：等级定义改由插件设置维护（后台“插件设置” → classes），本表是设置的物化结果。
-- key（slug）是等级的唯一标识；数字 id 只作内部引用（identity 的 class_id、角色 levels:class_<id>），保持稳定。
ALTER TABLE classes DROP CONSTRAINT classes_type_check;
ALTER TABLE classes ADD CONSTRAINT classes_type_check CHECK (type IN ('normal', 'vip', 'honor', 'staff'));
ALTER TABLE classes ADD COLUMN name text NOT NULL DEFAULT '';
ALTER TABLE classes ADD COLUMN description text NOT NULL DEFAULT '';
ALTER TABLE classes ALTER COLUMN slug SET NOT NULL;

-- 设置同步状态（单行）：hash = 最近一次处理的设置摘要；error = 设置无效时的原因（保留上一份有效定义）；
-- full_pending = 定义变化后需要一次全量评估（下一次 incr_evaluate 执行）
CREATE TABLE sync_state (
    id            boolean PRIMARY KEY DEFAULT true CHECK (id),
    hash          text NOT NULL DEFAULT '',
    error         text,
    full_pending  boolean NOT NULL DEFAULT false,
    synced_at     timestamptz NOT NULL DEFAULT now()
);
INSERT INTO sync_state DEFAULT VALUES;

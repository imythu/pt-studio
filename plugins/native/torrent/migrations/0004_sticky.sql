-- 置顶：sticky_until 之前在列表中排在前面（永久置顶为 9999-12-31）；到期由 `unsticky` 任务清除并触发重建索引
ALTER TABLE torrents ADD COLUMN sticky_until timestamptz;
CREATE INDEX torrents_sticky_idx ON torrents (sticky_until) WHERE sticky_until IS NOT NULL;

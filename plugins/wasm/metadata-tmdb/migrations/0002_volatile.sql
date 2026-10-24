-- 近期上映 / 连载中的条目缓存较短（LLD §1.4）
ALTER TABLE cache ADD COLUMN volatile boolean NOT NULL DEFAULT false;

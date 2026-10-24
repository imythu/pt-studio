-- metadata-musicbrainz（共用设计 CMN-META §3；发行组至少缓存 90 天，按 external_id 前缀判断）
CREATE TABLE cache (
    external_id   text NOT NULL,
    locale        text NOT NULL,
    data          jsonb,
    not_found     boolean NOT NULL DEFAULT false,
    content_hash  text,
    fetched_at    timestamptz NOT NULL,
    last_used_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (external_id, locale)
);
CREATE INDEX cache_refresh_idx ON cache (fetched_at) WHERE NOT not_found;

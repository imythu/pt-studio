-- metadata-bangumi（共用设计 CMN-META §3；表结构同 metadata-tmdb）
CREATE TABLE cache (
    external_id   text NOT NULL,
    locale        text NOT NULL,
    data          jsonb,
    not_found     boolean NOT NULL DEFAULT false,
    content_hash  text,
    volatile      boolean NOT NULL DEFAULT false,
    fetched_at    timestamptz NOT NULL,
    last_used_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (external_id, locale)
);
CREATE INDEX cache_refresh_idx ON cache (fetched_at) WHERE NOT not_found;

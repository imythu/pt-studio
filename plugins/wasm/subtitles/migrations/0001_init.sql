-- 字幕：文件存 blob（`files/<sha256>`，非公开，经接口下载），数据库存元数据
CREATE TABLE subtitles (
    id            bigserial PRIMARY KEY,
    torrent_id    bigint NOT NULL,
    uploader_id   bigint NOT NULL,
    is_anonymous  boolean NOT NULL DEFAULT false,
    lang          text NOT NULL,
    title         text NOT NULL DEFAULT '' CHECK (char_length(title) <= 200),
    filename      text NOT NULL CHECK (char_length(filename) BETWEEN 1 AND 200),
    format        text NOT NULL,
    encoding      text,
    entries       jsonb NOT NULL DEFAULT '[]',
    size          integer NOT NULL,
    sha256        bytea NOT NULL,
    blob_key      text NOT NULL,
    downloads     integer NOT NULL DEFAULT 0,
    created_at    timestamptz NOT NULL DEFAULT now(),
    deleted_at    timestamptz,
    deleted_by    bigint,
    delete_reason text
);
CREATE INDEX subtitles_torrent_idx ON subtitles (torrent_id) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX subtitles_torrent_sha_idx ON subtitles (torrent_id, sha256) WHERE deleted_at IS NULL;
CREATE INDEX subtitles_uploader_idx ON subtitles (uploader_id, created_at DESC);
CREATE INDEX subtitles_time_idx ON subtitles (created_at DESC);

-- mediainfo（docs/plugins/mediainfo/03 §3）：每个种子一份解析结果；只保存脱敏后的原文
CREATE TABLE results (
    torrent_id      bigint PRIMARY KEY,
    text            text NOT NULL,
    text_hash       bytea NOT NULL,
    format          text NOT NULL CHECK (format IN ('text', 'json', 'bdinfo')),
    summary         jsonb NOT NULL,
    sections        jsonb NOT NULL,
    diffs           jsonb NOT NULL DEFAULT '[]',
    parser_version  integer NOT NULL,
    updated_by      bigint,
    parsed_at       timestamptz NOT NULL DEFAULT now()
);

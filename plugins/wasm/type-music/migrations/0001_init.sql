-- type-music（LLD §3）：抓轨日志检查结果，每个 log 一行
CREATE TABLE log_results (
    torrent_id       bigint NOT NULL,
    log_index        smallint NOT NULL,
    software         text,
    software_version text,
    score            smallint,
    issues           jsonb NOT NULL DEFAULT '[]',
    checker_version  integer NOT NULL,
    checked_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (torrent_id, log_index)
);

-- 种子视角（FR-ACH-042）：复核后确认基线，之后的交叉校验异常重新计数
CREATE TABLE torrent_baselines (
    torrent_id    bigint PRIMARY KEY,
    confirmed_at  timestamptz NOT NULL,
    confirmed_by  bigint NOT NULL,
    note          text CHECK (char_length(note) <= 500)
);

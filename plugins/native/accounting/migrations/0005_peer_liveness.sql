-- peer 存活判定（修复：客户端崩溃 / 重启换 peer_id / 删种未发 stopped 时，旧 peer 一直 is_active，
-- 种子的做种 / 下载人数与 peer 列表里长期留着早已离线的 peer）。
-- 活跃 = is_active 且最近 1 小时内汇报过（大于 tracker 的 max_peer_timeout 默认 2700 s）；
-- 定时任务 reap_peers 把超时的 peer 置为非活跃，这里的时间条件保证两次清理之间也不计入。
CREATE INDEX peers_active_announce_idx ON peers (last_announce_at) WHERE is_active;

CREATE OR REPLACE VIEW v_torrent_peers WITH (security_barrier = true) AS
SELECT t.id AS torrent_id,
       coalesce(p.seeders, 0) AS seeders,
       coalesce(p.leechers, 0) AS leechers,
       coalesce(s.completed, 0) AS completed
FROM torrents t
LEFT JOIN (SELECT torrent_id, count(*) FILTER (WHERE is_seeder) AS seeders, count(*) FILTER (WHERE NOT is_seeder) AS leechers
             FROM peers WHERE is_active AND last_announce_at > now() - interval '1 hour' GROUP BY torrent_id) p
       ON p.torrent_id = t.id
LEFT JOIN (SELECT torrent_id, count(*) AS completed FROM snatches WHERE completed_at IS NOT NULL GROUP BY torrent_id) s
       ON s.torrent_id = t.id;

-- 存量：已经超时的 peer 直接置为非活跃
UPDATE peers SET is_active = false WHERE is_active AND last_announce_at < now() - interval '1 hour';

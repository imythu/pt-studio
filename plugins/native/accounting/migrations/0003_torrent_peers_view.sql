-- 每个种子的做种 / 下载人数与完成数（rss 的 Torznab 属性等只读消费者使用）
CREATE VIEW v_torrent_peers WITH (security_barrier = true) AS
SELECT t.id AS torrent_id,
       coalesce(p.seeders, 0) AS seeders,
       coalesce(p.leechers, 0) AS leechers,
       coalesce(s.completed, 0) AS completed
FROM torrents t
LEFT JOIN (SELECT torrent_id, count(*) FILTER (WHERE is_seeder) AS seeders, count(*) FILTER (WHERE NOT is_seeder) AS leechers
             FROM peers WHERE is_active GROUP BY torrent_id) p ON p.torrent_id = t.id
LEFT JOIN (SELECT torrent_id, count(*) AS completed FROM snatches WHERE completed_at IS NOT NULL GROUP BY torrent_id) s
       ON s.torrent_id = t.id;

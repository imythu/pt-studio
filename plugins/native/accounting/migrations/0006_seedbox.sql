-- 盒子标记：peer 来自 seedbox 插件已批准的网段（写入时按记账流中的 IP 判断，IP 不落库）
ALTER TABLE peers ADD COLUMN is_seedbox boolean NOT NULL DEFAULT false;

CREATE OR REPLACE VIEW v_swarm_peers WITH (security_barrier = true) AS
SELECT p.torrent_id, p.user_id, p.is_seeder, p.client_prefix, p.first_seen_at, p.last_announce_at,
       coalesce(s.uploaded_raw, 0) AS uploaded_raw, coalesce(s.downloaded_raw, 0) AS downloaded_raw,
       p.is_seedbox
FROM peers p
LEFT JOIN snatches s ON s.user_id = p.user_id AND s.torrent_id = p.torrent_id
WHERE p.is_active AND p.last_announce_at > now() - interval '1 hour';

-- 审核类插件（ai-upload-review 等）读取的种子全貌：标题、描述、MediaInfo、字段、最新文件列表
CREATE VIEW v_torrent_review WITH (security_barrier = true) AS
SELECT t.id, t.group_id, t.type_id, t.title, t.subtitle, t.description, t.mediainfo, t.fields, t.size, t.file_count, t.has_nfo,
       t.status, t.version, t.created_at, f.files
FROM torrents t
LEFT JOIN torrent_files f ON f.torrent_id = t.id AND f.file_version = t.file_version
WHERE t.deleted_at IS NULL;

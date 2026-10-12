-- 作品（Group）只读视图（资源类型共用设计 §9）
CREATE VIEW v_group_public WITH (security_barrier = true) AS
SELECT id, type_id, group_key, title, year, torrent_count, last_upload_at, created_at
FROM groups WHERE deleted_at IS NULL AND merged_into IS NULL;

CREATE VIEW v_group_fields WITH (security_barrier = true) AS
SELECT id AS group_id, type_id, group_key, fields, updated_at
FROM groups WHERE deleted_at IS NULL AND merged_into IS NULL;

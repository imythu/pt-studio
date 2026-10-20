-- 预置分区与版块：首次加载时站点即可使用论坛（已有分区的站点不变）。权限为空表示所有能使用论坛的用户可读写。
WITH cat AS (
    INSERT INTO categories (name, sort)
    SELECT v.name, v.sort FROM (VALUES ('站务', 10), ('资源交流', 20), ('休闲', 30)) AS v (name, sort)
     WHERE NOT EXISTS (SELECT 1 FROM categories)
    RETURNING id, name
)
INSERT INTO boards (category_id, name, description, sort, acl)
SELECT cat.id, b.name, b.description, b.sort, b.acl::jsonb
  FROM cat JOIN (VALUES
    ('站务', '站务公告', '站点规则、公告与活动（仅管理组可发帖）', 10, '{"create": {"min_class": 1000, "roles": ["moderator", "admin", "sysop"]}}'),
    ('站务', '意见建议', '对站点的建议与问题反馈', 20, '{}'),
    ('站务', '新手求助', '使用问题、客户端设置、做种与分享率', 30, '{}'),
    ('资源交流', '求种与资源讨论', '资源推荐、版本对比、求种互助', 10, '{}'),
    ('资源交流', '发布组与压制', '发布组动态、压制与制作技术交流', 20, '{}'),
    ('休闲', '灌水区', '随便聊聊', 10, '{}')
  ) AS b (cat, name, description, sort, acl) ON b.cat = cat.name;

-- 私信举报理由
INSERT INTO reasons (kind, code, priority, requires_note) VALUES
    ('message', 'spam', 20, false), ('message', 'abuse', 50, false), ('message', 'scam', 80, false), ('message', 'other', 20, true)
ON CONFLICT DO NOTHING;

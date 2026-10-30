-- 多账号关联：复核员对同网段账号对的结论（user_a < user_b）。
-- same_person = 确认同一人；unrelated = 确认无关（如校园网 / 公司 NAT），之后的同网段对端证据跳过该对
CREATE TABLE account_links (
    user_a       bigint NOT NULL,
    user_b       bigint NOT NULL,
    verdict      text NOT NULL CHECK (verdict IN ('same_person', 'unrelated')),
    note         text CHECK (char_length(note) <= 500),
    reviewed_by  bigint NOT NULL,
    reviewed_at  timestamptz NOT NULL,
    PRIMARY KEY (user_a, user_b),
    CHECK (user_a < user_b)
);
CREATE INDEX account_links_user_b_idx ON account_links (user_b);

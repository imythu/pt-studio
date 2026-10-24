-- 客户端指纹（FR-ACC-019）：peer 最近一次 User-Agent 所属客户端族（不存 UA 原文），用于识别同一 peer 中途更换客户端
ALTER TABLE peers ADD COLUMN ua_family text CHECK (char_length(ua_family) <= 32);

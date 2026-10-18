-- 新商品类型：VIP 天数（ctx.levels.grant_vip，在已有 VIP 之后顺延）
ALTER TABLE items DROP CONSTRAINT items_kind_check;
ALTER TABLE items ADD CONSTRAINT items_kind_check CHECK (kind IN ('upload_credit', 'invite', 'promo_card', 'hnr_removal', 'medal', 'vip'));

-- 新商品类型：勋章 / 头衔（ctx.medals.grant）
ALTER TABLE items DROP CONSTRAINT items_kind_check;
ALTER TABLE items ADD CONSTRAINT items_kind_check CHECK (kind IN ('upload_credit', 'invite', 'promo_card', 'hnr_removal', 'medal'));

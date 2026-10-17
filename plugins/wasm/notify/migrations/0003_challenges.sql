-- identity 邮件挑战（邮箱验证、找回密码）：outbox 只记挑战 ID，链接与收件人在投递时经 ctx.auth.challenge_delivery 取得，不落库
ALTER TABLE outbox ADD COLUMN challenge_id uuid;

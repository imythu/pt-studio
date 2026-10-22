-- ledger 0003：批量扣除（docs/plugins/ledger/03 §1.12）。search_path = ledger。
ALTER TABLE grant_batches
    ADD COLUMN direction  text NOT NULL DEFAULT 'grant' CHECK (direction IN ('grant','deduct')),
    -- 扣除时余额不足：skip = 跳过此人；partial = 扣到可用余额为 0
    ADD COLUMN shortfall  text NOT NULL DEFAULT 'skip' CHECK (shortfall IN ('skip','partial')),
    ADD COLUMN moved      bigint NOT NULL DEFAULT 0;   -- 实际发放 / 扣除总额（最小单位）

ALTER TABLE grant_recipients
    ADD COLUMN amount  bigint;                          -- 此人实际发放 / 扣除的数量

UPDATE grant_batches SET moved = amount * granted;
UPDATE grant_recipients r SET amount = b.amount FROM grant_batches b WHERE b.id = r.batch_id AND r.entry_id IS NOT NULL;

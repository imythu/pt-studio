-- levels 0003：等级简写（如 PU、EU），为空时用内置的常用简写
ALTER TABLE classes ADD COLUMN abbr text NOT NULL DEFAULT '';

-- llm-claude（LLD §3）：每日用量汇总
CREATE TABLE usage_daily (
    day                 date NOT NULL,
    caller_plugin       text NOT NULL,
    model               text NOT NULL,
    calls               bigint NOT NULL DEFAULT 0,
    input_tokens        bigint NOT NULL DEFAULT 0,
    output_tokens       bigint NOT NULL DEFAULT 0,
    cached_input_tokens bigint NOT NULL DEFAULT 0,
    refusals            bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (day, caller_plugin, model)
);

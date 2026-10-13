//! 结构化日志（转发到 `pt:plugin/log`，内核自动附带 `plugin_id` 与 `trace_id`）：
//! `logging::info("settled", &[("users", &n.to_string())])`。

use crate::log::{self, Field, Level};

fn emit(level: Level, message: &str, fields: &[(&str, &str)]) {
    let fields: Vec<Field> = fields
        .iter()
        .map(|(k, v)| Field {
            key: (*k).to_owned(),
            value: (*v).to_owned(),
        })
        .collect();
    log::emit(level, message, &fields);
}

pub fn debug(message: &str, fields: &[(&str, &str)]) {
    emit(Level::Debug, message, fields);
}

pub fn info(message: &str, fields: &[(&str, &str)]) {
    emit(Level::Info, message, fields);
}

pub fn warn(message: &str, fields: &[(&str, &str)]) {
    emit(Level::Warn, message, fields);
}

pub fn error(message: &str, fields: &[(&str, &str)]) {
    emit(Level::Error, message, fields);
}

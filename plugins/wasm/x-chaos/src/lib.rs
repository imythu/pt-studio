//! 故意越界的测试插件：验证沙箱的截止时间、陷入、内存上限与能力隔离。

pt_plugin_sdk::world!("chaos");

use exports::pt::plugin::http_handler::{Guest, Request, Response};
use pt_plugin_sdk::{redis, sql, types::PluginError};

struct Chaos;

fn ok(body: &str) -> Response {
    Response {
        status: 200,
        headers: vec![],
        body: body.as_bytes().to_vec(),
    }
}

impl Guest for Chaos {
    fn handle(req: Request) -> Result<Response, PluginError> {
        let op = req
            .params
            .iter()
            .find(|(k, _)| k == "op")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        match op {
            "loop" => {
                let mut x: u64 = 0;
                loop {
                    x = x.wrapping_add(1);
                    std::hint::black_box(x);
                }
            }
            "panic" => panic!("boom"),
            "alloc" => {
                let v = vec![7u8; 64 << 20];
                Ok(ok(&v.len().to_string()))
            }
            "peek" => sql::query("SELECT count(*) FROM identity.users", &[]).map(|_| ok("read")),
            "eval" => redis::call(&redis::Command {
                name: "EVAL".into(),
                args: vec![b"return 1".to_vec(), b"0".to_vec()],
            })
            .map(|_| ok("eval")),
            "fine" => Ok(ok("fine")),
            _ => Err(PluginError {
                code: "kernel.not_found".into(),
                message: "no op".into(),
                details: None,
                retryable: false,
            }),
        }
    }
}

export!(Chaos);

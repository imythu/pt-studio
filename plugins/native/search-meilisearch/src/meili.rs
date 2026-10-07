//! Meilisearch HTTP 客户端（LLD §1.1 meili_client）：API key 只在本模块出现。

use std::time::Duration;

use pt_plugin_api::{PResult, PluginError};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct Meili {
    pub id: String,
    url: String,
    key: String,
    client: reqwest::Client,
    request_timeout: Duration,
    write_timeout: Duration,
}

fn unavailable(msg: impl std::fmt::Display) -> PluginError {
    PluginError::new("search-meilisearch.unavailable", format!("meilisearch: {msg}")).retryable()
}

impl Meili {
    pub fn new(
        id: &str,
        url: &str,
        key: &str,
        request_timeout_ms: u64,
        write_timeout_ms: u64,
    ) -> PResult<Self> {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(64)
            .build()
            .map_err(|e| PluginError::internal(format!("http client: {e}")))?;
        Ok(Self {
            id: id.into(),
            url: url.trim_end_matches('/').into(),
            key: key.into(),
            client,
            request_timeout: Duration::from_millis(request_timeout_ms),
            write_timeout: Duration::from_millis(write_timeout_ms),
        })
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        timeout: Duration,
    ) -> PResult<(StatusCode, Value)> {
        let mut req = self
            .client
            .request(method, format!("{}{path}", self.url))
            .timeout(timeout);
        if !self.key.is_empty() {
            req = req.bearer_auth(&self.key);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(unavailable)?;
        let status = resp.status();
        let text = resp.text().await.map_err(unavailable)?;
        let v = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        if status.is_server_error() {
            return Err(unavailable(format!("{status}: {v}")));
        }
        Ok((status, v))
    }

    /// 写操作：返回 taskUid
    async fn task(&self, method: Method, path: &str, body: Option<&Value>) -> PResult<u64> {
        let (status, v) = self.send(method, path, body, self.write_timeout).await?;
        if !status.is_success() {
            return Err(PluginError::internal(format!("meilisearch {path}: {status} {v}")));
        }
        v["taskUid"]
            .as_u64()
            .ok_or_else(|| PluginError::internal(format!("meilisearch {path}: no taskUid in {v}")))
    }

    pub async fn version(&self) -> PResult<String> {
        let (_, v) = self
            .send(Method::GET, "/version", None, self.request_timeout)
            .await?;
        Ok(v["pkgVersion"].as_str().unwrap_or_default().to_owned())
    }

    pub async fn index_exists(&self, uid: &str) -> PResult<bool> {
        let (status, v) = self
            .send(
                Method::GET,
                &format!("/indexes/{uid}"),
                None,
                self.request_timeout,
            )
            .await?;
        match status {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            _ => Err(PluginError::internal(format!(
                "meilisearch get index: {status} {v}"
            ))),
        }
    }

    pub async fn create_index(&self, uid: &str) -> PResult<u64> {
        self.task(
            Method::POST,
            "/indexes",
            Some(&json!({ "uid": uid, "primaryKey": "id" })),
        )
        .await
    }

    pub async fn delete_index(&self, uid: &str) -> PResult<u64> {
        self.task(Method::DELETE, &format!("/indexes/{uid}"), None).await
    }

    pub async fn patch_settings(&self, uid: &str, settings: &Value) -> PResult<u64> {
        self.task(Method::PATCH, &format!("/indexes/{uid}/settings"), Some(settings))
            .await
    }

    /// 按主键合并（PUT）
    pub async fn update_documents(&self, uid: &str, docs: &[Value]) -> PResult<u64> {
        self.task(
            Method::PUT,
            &format!("/indexes/{uid}/documents?primaryKey=id"),
            Some(&Value::Array(docs.to_vec())),
        )
        .await
    }

    pub async fn delete_documents(&self, uid: &str, ids: &[i64]) -> PResult<u64> {
        self.task(
            Method::POST,
            &format!("/indexes/{uid}/documents/delete-batch"),
            Some(&json!(ids)),
        )
        .await
    }

    pub async fn swap(&self, a: &str, b: &str) -> PResult<u64> {
        self.task(
            Method::POST,
            "/swap-indexes",
            Some(&json!([{ "indexes": [a, b] }])),
        )
        .await
    }

    pub async fn document_count(&self, uid: &str) -> PResult<u64> {
        let (status, v) = self
            .send(
                Method::GET,
                &format!("/indexes/{uid}/stats"),
                None,
                self.request_timeout,
            )
            .await?;
        if !status.is_success() {
            return Err(PluginError::internal(format!("meilisearch stats: {status} {v}")));
        }
        Ok(v["numberOfDocuments"].as_u64().unwrap_or(0))
    }

    /// 任务状态：enqueued / processing / succeeded / failed / canceled，及错误
    pub async fn task_status(&self, task: u64) -> PResult<(String, Option<Value>)> {
        let (status, v) = self
            .send(Method::GET, &format!("/tasks/{task}"), None, self.request_timeout)
            .await?;
        if !status.is_success() {
            return Err(PluginError::internal(format!(
                "meilisearch task {task}: {status} {v}"
            )));
        }
        Ok((
            v["status"].as_str().unwrap_or("").to_owned(),
            v.get("error").cloned().filter(|e| !e.is_null()),
        ))
    }

    /// 等待任务结束；失败返回错误，超时返回 `Ok(false)`
    pub async fn wait(&self, task: u64, max: Duration) -> PResult<bool> {
        let start = std::time::Instant::now();
        let mut step = Duration::from_millis(20);
        loop {
            let (s, e) = self.task_status(task).await?;
            match s.as_str() {
                "succeeded" => return Ok(true),
                "failed" | "canceled" => {
                    return Err(PluginError::internal(format!(
                        "meilisearch task {task} {s}: {}",
                        e.unwrap_or_default()
                    )));
                }
                _ => {}
            }
            if start.elapsed() >= max {
                return Ok(false);
            }
            tokio::time::sleep(step).await;
            step = (step * 2).min(Duration::from_millis(500));
        }
    }

    pub async fn search(&self, uid: &str, body: &Value) -> PResult<Value> {
        let (status, v) = self
            .send(
                Method::POST,
                &format!("/indexes/{uid}/search"),
                Some(body),
                self.request_timeout,
            )
            .await?;
        if status == StatusCode::NOT_FOUND {
            return Err(PluginError::new(
                "search-meilisearch.index_not_found",
                "index is not ready",
            ));
        }
        if !status.is_success() {
            return Err(PluginError::internal(format!("meilisearch search: {status} {v}")));
        }
        Ok(v)
    }
}

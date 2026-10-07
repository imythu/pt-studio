//! 对象存储网关（kernel/03 §6.7）。当前实现本地文件系统后端（`[blob] dir`，开发与单机部署）；
//! S3 兼容后端在后续迭代加入。键改写为 `plugins/<id>/<key>`。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use pt_plugin_api::{PResult, PluginError, host::Blob};

#[derive(Debug, Clone)]
pub struct FsBlob {
    enabled: bool,
    root: Option<PathBuf>,
    plugin: String,
}

/// 键规则：`^[A-Za-z0-9!_.*'()/-]{1,512}$`，不含 `..`、不以 `/` 开头。
pub fn valid_key(key: &str) -> bool {
    (1..=512).contains(&key.len())
        && !key.starts_with('/')
        && !key.contains("..")
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"!_.*'()/-".contains(&c))
}

impl FsBlob {
    pub fn new(enabled: bool, root: Option<&str>, plugin: &str) -> Self {
        Self {
            enabled,
            root: root.map(PathBuf::from),
            plugin: plugin.to_owned(),
        }
    }

    fn path(&self, key: &str) -> PResult<PathBuf> {
        if !self.enabled {
            return Err(PluginError::capability_denied("capability `blob` not declared"));
        }
        let root = self
            .root
            .as_ref()
            .ok_or_else(|| PluginError::unavailable("object storage is not configured"))?;
        if !valid_key(key) {
            return Err(PluginError::bad_request(format!("invalid blob key `{key}`")));
        }
        Ok(root.join("plugins").join(&self.plugin).join(key))
    }
}

async fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::now_v7().simple()));
    tokio::fs::write(&tmp, data).await?;
    tokio::fs::rename(&tmp, path).await
}

#[async_trait]
impl Blob for FsBlob {
    async fn put(&self, key: &str, _content_type: &str, data: bytes::Bytes) -> PResult<()> {
        let p = self.path(key)?;
        write_atomic(&p, &data)
            .await
            .map_err(|e| PluginError::unavailable(format!("blob write failed: {e}")))
    }

    async fn get(&self, key: &str) -> PResult<Option<bytes::Bytes>> {
        let p = self.path(key)?;
        match tokio::fs::read(&p).await {
            Ok(v) => Ok(Some(v.into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(PluginError::unavailable(format!("blob read failed: {e}"))),
        }
    }

    async fn delete(&self, key: &str) -> PResult<()> {
        let p = self.path(key)?;
        match tokio::fs::remove_file(&p).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(PluginError::unavailable(format!("blob delete failed: {e}"))),
        }
    }

    async fn presign_get(&self, _key: &str, _ttl_secs: u32) -> PResult<String> {
        Err(PluginError::unavailable(
            "presigned URLs require an S3-compatible backend",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert!(valid_key("torrents/ab12.torrent"));
        assert!(!valid_key("../x"));
        assert!(!valid_key("/abs"));
        assert!(!valid_key("a b"));
        assert!(!valid_key(""));
    }
}

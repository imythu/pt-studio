//! WebAuthn（identity 03 §1.4）：安全密钥 / 平台认证器，作为第二因素与免密码登录。
//!
//! 偏差：设计使用 webauthn-rs；本版本自行实现最小子集——注册只接受 `attestation: "none"` 语义（不校验证明链，只取公钥），
//! 支持 ES256（P-256）、EdDSA（Ed25519）、RS256；仪式状态存 Redis（5 分钟，GETDEL 只能完成一次）。

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pt_plugin_api::{Ctx, NewEvent, PResult, PluginError, RedisCommand, RedisReply, json};
use serde_json::Value;
use sqlx::Row;

use crate::{
    Identity,
    auth::{self, NewSession},
    cbor, crypto,
    policy::Policy,
    risk, two_factor,
};

const CEREMONY_TTL_SECS: u32 = 300;
const MAX_CREDENTIALS: i64 = 10;
const ALG_ES256: i128 = -7;
const ALG_EDDSA: i128 = -8;
const ALG_RS256: i128 = -257;

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_AT: u8 = 0x40;

fn b64d(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok()
}

fn invalid(msg: &str) -> PluginError {
    PluginError::new("identity.webauthn_invalid", msg.to_owned()).with_status(400)
}

/// 依赖方：rp_id 为站点地址的主机名，origin 为站点地址的 scheme://host[:port]
struct Rp {
    id: String,
    origin: String,
    name: String,
}

fn rp(ctx: &Ctx) -> PResult<Rp> {
    let st = ctx.config.get_static();
    let base = st["webauthn_origin"]
        .as_str()
        .or_else(|| st["_site"]["base_url"].as_str())
        .unwrap_or("");
    let (scheme, rest) = base
        .split_once("://")
        .ok_or_else(|| PluginError::unavailable("site.base_url is not configured"))?;
    let hostport = rest.split('/').next().unwrap_or("");
    let host = hostport.rsplit_once(':').map_or(hostport, |(h, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) { h } else { hostport }
    });
    Ok(Rp {
        id: st["webauthn_rp_id"].as_str().unwrap_or(host).to_owned(),
        origin: format!("{scheme}://{hostport}"),
        name: st["_site"]["name"].as_str().unwrap_or("PT").to_owned(),
    })
}

fn uid_of(ctx: &Ctx) -> PResult<i64> {
    i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))
}

async fn redis_put(ctx: &Ctx, key: &str, v: &Value) -> PResult<()> {
    ctx.redis
        .call(
            RedisCommand::new("SET")
                .arg(key)
                .arg(v.to_string())
                .arg("EX")
                .arg(CEREMONY_TTL_SECS.to_string()),
        )
        .await?;
    Ok(())
}

async fn redis_take(ctx: &Ctx, key: &str) -> PResult<Option<Value>> {
    Ok(match ctx.redis.call(RedisCommand::new("GETDEL").arg(key)).await? {
        RedisReply::Bulk(b) => serde_json::from_slice(&b).ok(),
        _ => None,
    })
}

/// WebAuthn 的 user.id：每个用户一个随机 32 字节句柄（不暴露数字 ID）
async fn user_handle(ctx: &Ctx, uid: i64) -> PResult<Vec<u8>> {
    let mut conn = ctx.sql.conn().await?;
    if let Some(h) = sqlx::query_scalar::<_, Vec<u8>>("SELECT user_handle FROM webauthn_user_handles WHERE user_id = $1")
        .bind(uid)
        .fetch_optional(&mut *conn)
        .await?
    {
        return Ok(h);
    }
    let h = ctx.random.bytes(32);
    sqlx::query("INSERT INTO webauthn_user_handles (user_id, user_handle) VALUES ($1, $2) ON CONFLICT (user_id) DO NOTHING")
        .bind(uid)
        .bind(&h)
        .execute(&mut *conn)
        .await?;
    Ok(sqlx::query_scalar("SELECT user_handle FROM webauthn_user_handles WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&mut *conn)
        .await?)
}

pub async fn count(ctx: &Ctx, uid: i64) -> PResult<i64> {
    let mut conn = ctx.sql.conn().await?;
    Ok(sqlx::query_scalar("SELECT count(*) FROM webauthn_credentials WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&mut *conn)
        .await?)
}

pub async fn credentials_of(ctx: &Ctx, uid: i64) -> PResult<Vec<Value>> {
    let mut conn = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT id, name, discoverable, created_at, last_used_at FROM webauthn_credentials WHERE user_id = $1 ORDER BY id",
    )
    .bind(uid)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({ "id": r.get::<i64, _>(0), "name": r.get::<String, _>(1), "discoverable": r.get::<bool, _>(2),
                    "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>(3).timestamp_millis(),
                    "last_used_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>(4).map(|t| t.timestamp_millis()) })
        })
        .collect())
}

// ---------------- 认证器数据与公钥 ----------------

struct AuthData {
    rp_id_hash: Vec<u8>,
    flags: u8,
    sign_count: u32,
    /// (aaguid, credential id, COSE 公钥原始字节)
    attested: Option<(Vec<u8>, Vec<u8>, Vec<u8>)>,
}

fn parse_auth_data(d: &[u8]) -> Option<AuthData> {
    if d.len() < 37 {
        return None;
    }
    let flags = d[32];
    let sign_count = u32::from_be_bytes(d[33..37].try_into().ok()?);
    let attested = if flags & FLAG_AT != 0 {
        let aaguid = d.get(37..53)?.to_vec();
        let len = usize::from(u16::from_be_bytes(d.get(53..55)?.try_into().ok()?));
        let cred_id = d.get(55..55 + len)?.to_vec();
        let (_, used) = cbor::decode(d.get(55 + len..)?)?;
        let key = d.get(55 + len..55 + len + used)?.to_vec();
        Some((aaguid, cred_id, key))
    } else {
        None
    };
    Some(AuthData {
        rp_id_hash: d[..32].to_vec(),
        flags,
        sign_count,
        attested,
    })
}

/// 校验 COSE 公钥（支持的算法与必要字段），返回算法
fn cose_alg(key: &[u8]) -> Option<i128> {
    let (k, _) = cbor::decode(key)?;
    let alg = k.get_int(3)?.as_int()?;
    let ok = match alg {
        ALG_ES256 => {
            k.get_int(1)?.as_int()? == 2
                && k.get_int(-1)?.as_int()? == 1
                && k.get_int(-2)?.as_bytes()?.len() == 32
                && k.get_int(-3)?.as_bytes()?.len() == 32
        }
        ALG_EDDSA => {
            k.get_int(1)?.as_int()? == 1 && k.get_int(-1)?.as_int()? == 6 && k.get_int(-2)?.as_bytes()?.len() == 32
        }
        ALG_RS256 => {
            k.get_int(1)?.as_int()? == 3
                && k.get_int(-1)?.as_bytes()?.len() >= 256
                && !k.get_int(-2)?.as_bytes()?.is_empty()
        }
        _ => false,
    };
    ok.then_some(alg)
}

/// 用 COSE 公钥验证签名
fn verify_sig(key: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let Some((k, _)) = cbor::decode(key) else { return false };
    let int = |i| k.get_int(i).and_then(cbor::Value::as_bytes);
    match k.get_int(3).and_then(cbor::Value::as_int) {
        Some(ALG_ES256) => {
            use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
            let (Some(x), Some(y)) = (int(-2), int(-3)) else { return false };
            let mut point = vec![0x04];
            point.extend_from_slice(x);
            point.extend_from_slice(y);
            match (VerifyingKey::from_sec1_bytes(&point), Signature::from_der(sig)) {
                (Ok(vk), Ok(s)) => vk.verify(msg, &s).is_ok(),
                _ => false,
            }
        }
        Some(ALG_EDDSA) => {
            use ed25519_dalek::{Signature, Verifier, VerifyingKey};
            let Some(x) = int(-2).and_then(|x| <[u8; 32]>::try_from(x).ok()) else { return false };
            match (VerifyingKey::from_bytes(&x), Signature::from_slice(sig)) {
                (Ok(vk), Ok(s)) => vk.verify(msg, &s).is_ok(),
                _ => false,
            }
        }
        Some(ALG_RS256) => {
            use rsa::{BigUint, RsaPublicKey, pkcs1v15, signature::Verifier};
            let (Some(n), Some(e)) = (int(-1), int(-2)) else { return false };
            let Ok(pk) = RsaPublicKey::new(BigUint::from_bytes_be(n), BigUint::from_bytes_be(e)) else {
                return false;
            };
            let vk = pkcs1v15::VerifyingKey::<sha2::Sha256>::new(pk);
            match pkcs1v15::Signature::try_from(sig) {
                Ok(s) => vk.verify(msg, &s).is_ok(),
                Err(_) => false,
            }
        }
        _ => false,
    }
}

/// 校验 clientDataJSON：类型、挑战、来源
fn check_client_data(raw: &[u8], typ: &str, challenge: &str, origin: &str) -> PResult<()> {
    let c: Value = serde_json::from_slice(raw).map_err(|_| invalid("bad clientDataJSON"))?;
    if c["type"] != typ {
        return Err(invalid("wrong ceremony type"));
    }
    if c["challenge"].as_str().map(|s| s.trim_end_matches('=')) != Some(challenge) {
        return Err(invalid("challenge mismatch"));
    }
    if c["origin"] != origin {
        return Err(invalid("origin mismatch"));
    }
    Ok(())
}

fn field<'a>(v: &'a Value, path: &[&str]) -> PResult<&'a str> {
    let mut cur = v;
    for p in path {
        cur = &cur[*p];
    }
    cur.as_str()
        .ok_or_else(|| invalid(&format!("missing {}", path.join("."))))
}

// ---------------- 注册 ----------------

/// 登记选项（navigator.credentials.create 的 publicKey，二进制字段为 base64url）
pub async fn registration_options(ctx: &Ctx) -> PResult<Value> {
    let uid = uid_of(ctx)?;
    if count(ctx, uid).await? >= MAX_CREDENTIALS {
        return Err(PluginError::new(
            "identity.webauthn_limit",
            "too many security keys",
        )
        .with_status(409));
    }
    let rp = rp(ctx)?;
    let handle = user_handle(ctx, uid).await?;
    let (username, existing): (String, Vec<Vec<u8>>) = {
        let mut conn = ctx.sql.conn().await?;
        let name = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
            .bind(uid)
            .fetch_one(&mut *conn)
            .await?;
        let ids = sqlx::query_scalar("SELECT credential_id FROM webauthn_credentials WHERE user_id = $1")
            .bind(uid)
            .fetch_all(&mut *conn)
            .await?;
        (name, ids)
    };
    let challenge = crypto::b64(&ctx.random.bytes(32));
    let ceremony = crypto::b64(&ctx.random.bytes(16));
    redis_put(
        ctx,
        &format!("wa:reg:{ceremony}"),
        &json!({ "uid": uid, "challenge": challenge }),
    )
    .await?;
    Ok(json!({
        "ceremony_id": ceremony,
        "public_key": {
            "rp": { "id": rp.id, "name": rp.name },
            "user": { "id": crypto::b64(&handle), "name": username, "displayName": username },
            "challenge": challenge,
            "pubKeyCredParams": [
                { "type": "public-key", "alg": ALG_ES256 },
                { "type": "public-key", "alg": ALG_EDDSA },
                { "type": "public-key", "alg": ALG_RS256 },
            ],
            "timeout": 120_000,
            "attestation": "none",
            "authenticatorSelection": { "residentKey": "preferred", "userVerification": "preferred" },
            "excludeCredentials": existing.iter().map(|id| json!({ "type": "public-key", "id": crypto::b64(id) })).collect::<Vec<_>>(),
            "extensions": { "credProps": true },
        }
    }))
}

/// 完成登记；若这是用户的第一个第二因素，同时返回一组恢复码
pub async fn register(ctx: &Ctx, ceremony: &str, name: &str, cred: &Value) -> PResult<Value> {
    let uid = uid_of(ctx)?;
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(PluginError::validation("name must be 1-64 characters"));
    }
    let st = redis_take(ctx, &format!("wa:reg:{ceremony}"))
        .await?
        .filter(|s| s["uid"] == uid)
        .ok_or_else(|| invalid("the registration has expired, please try again"))?;
    let rp = rp(ctx)?;
    let client_data = b64d(field(cred, &["response", "clientDataJSON"])?).ok_or_else(|| invalid("bad clientDataJSON"))?;
    check_client_data(
        &client_data,
        "webauthn.create",
        st["challenge"].as_str().unwrap_or(""),
        &rp.origin,
    )?;
    let att = b64d(field(cred, &["response", "attestationObject"])?).ok_or_else(|| invalid("bad attestationObject"))?;
    let (att, _) = cbor::decode(&att).ok_or_else(|| invalid("bad attestationObject"))?;
    let ad = att
        .get_text("authData")
        .and_then(cbor::Value::as_bytes)
        .and_then(parse_auth_data)
        .ok_or_else(|| invalid("bad authenticator data"))?;
    if !crypto::ct_eq(&ad.rp_id_hash, &crypto::sha256(rp.id.as_bytes())) {
        return Err(invalid("rp id mismatch"));
    }
    if ad.flags & FLAG_UP == 0 {
        return Err(invalid("user presence is required"));
    }
    let (aaguid, cred_id, key) = ad.attested.ok_or_else(|| invalid("no attested credential"))?;
    if b64d(field(cred, &["id"])?).as_deref() != Some(cred_id.as_slice()) {
        return Err(invalid("credential id mismatch"));
    }
    let alg = cose_alg(&key).ok_or_else(|| invalid("unsupported public key algorithm"))?;
    let discoverable = cred["clientExtensionResults"]["credProps"]["rk"] == true;
    let had_factor = two_factor::has_totp(ctx, uid).await? || count(ctx, uid).await? > 0;
    let id: i64 = {
        let mut conn = ctx.sql.conn().await?;
        let r = sqlx::query_scalar(
            "INSERT INTO webauthn_credentials (user_id, credential_id, public_key, alg, sign_count, name, discoverable, aaguid)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        )
        .bind(uid)
        .bind(&cred_id)
        .bind(&key)
        .bind(i32::try_from(alg).unwrap_or(0))
        .bind(i64::from(ad.sign_count))
        .bind(name)
        .bind(discoverable)
        .bind(uuid::Uuid::from_slice(&aaguid).ok())
        .fetch_one(&mut *conn)
        .await;
        match r {
            Ok(id) => id,
            Err(sqlx::Error::Database(db)) if db.constraint() == Some("webauthn_credentials_credential_id_key") => {
                return Err(PluginError::new(
                    "identity.webauthn_duplicate",
                    "this security key is already registered",
                )
                .with_status(409));
            }
            Err(e) => return Err(e.into()),
        }
    };
    let codes = if had_factor {
        None
    } else {
        Some(two_factor::regenerate_for(ctx, uid).await?)
    };
    risk::security_event(ctx, uid, "two_factor_enabled", json!({ "kind": "webauthn", "name": name })).await?;
    ctx.events
        .publish(NewEvent::v1(
            "user.two_factor_changed",
            format!("user:{uid}"),
            json!({ "user_id": uid, "kind": "webauthn", "enabled": true, "by": "user" }),
        ))
        .await?;
    Ok(json!({ "id": id, "discoverable": discoverable, "recovery_codes": codes }))
}

/// 删除一个安全密钥；角色要求 2FA 且这是最后一个第二因素时拒绝
pub async fn remove(ctx: &Ctx, cred_id: i64) -> PResult<()> {
    let uid = uid_of(ctx)?;
    let pol = Policy::load(ctx).await;
    if two_factor::required_for(ctx, &pol, uid).await?
        && !two_factor::has_totp(ctx, uid).await?
        && count(ctx, uid).await? <= 1
    {
        return Err(PluginError::new(
            "identity.two_factor_required",
            "your role requires two-factor authentication",
        )
        .with_status(403));
    }
    let n = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query("DELETE FROM webauthn_credentials WHERE id = $1 AND user_id = $2")
            .bind(cred_id)
            .bind(uid)
            .execute(&mut *conn)
            .await?
            .rows_affected()
    };
    if n == 0 {
        return Err(PluginError::not_found("security key not found"));
    }
    // 没有任何第二因素了：恢复码一并作废
    if !two_factor::has_totp(ctx, uid).await? && count(ctx, uid).await? == 0 {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
            .bind(uid)
            .execute(&mut *conn)
            .await?;
    }
    risk::security_event(ctx, uid, "two_factor_disabled", json!({ "kind": "webauthn" })).await?;
    ctx.events
        .publish(NewEvent::v1(
            "user.two_factor_changed",
            format!("user:{uid}"),
            json!({ "user_id": uid, "kind": "webauthn", "enabled": false, "by": "user" }),
        ))
        .await?;
    Ok(())
}

// ---------------- 断言 ----------------

/// 校验断言；`expect_uid` 为第二因素场景的用户（免密码登录为 None，按 userHandle 确认）。成功返回用户 ID。
async fn verify_assertion(
    ctx: &Ctx,
    challenge: &str,
    a: &Value,
    expect_uid: Option<i64>,
    require_uv: bool,
) -> PResult<Option<i64>> {
    let rp = rp(ctx)?;
    let raw_id = b64d(a["rawId"].as_str().or(a["id"].as_str()).unwrap_or("")).ok_or_else(|| invalid("bad credential id"))?;
    let row = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query(
            "SELECT c.id, c.user_id, c.public_key, c.sign_count, h.user_handle
               FROM webauthn_credentials c LEFT JOIN webauthn_user_handles h ON h.user_id = c.user_id
              WHERE c.credential_id = $1 FOR UPDATE OF c",
        )
        .bind(&raw_id)
        .fetch_optional(&mut *conn)
        .await?
    };
    let Some(row) = row else { return Ok(None) };
    let (cid, uid): (i64, i64) = (row.get(0), row.get(1));
    if expect_uid.is_some_and(|u| u != uid) {
        return Ok(None);
    }
    let client_data = b64d(field(a, &["response", "clientDataJSON"])?).ok_or_else(|| invalid("bad clientDataJSON"))?;
    check_client_data(&client_data, "webauthn.get", challenge, &rp.origin)?;
    let auth_data = b64d(field(a, &["response", "authenticatorData"])?).ok_or_else(|| invalid("bad authenticatorData"))?;
    let ad = parse_auth_data(&auth_data).ok_or_else(|| invalid("bad authenticator data"))?;
    if !crypto::ct_eq(&ad.rp_id_hash, &crypto::sha256(rp.id.as_bytes())) || ad.flags & FLAG_UP == 0 {
        return Ok(None);
    }
    if require_uv && ad.flags & FLAG_UV == 0 {
        return Err(invalid("user verification is required"));
    }
    if expect_uid.is_none() {
        let handle: Option<Vec<u8>> = row.get(4);
        let given = a["response"]["userHandle"].as_str().and_then(b64d);
        if handle.is_none() || given != handle {
            return Ok(None);
        }
    }
    let sig = b64d(field(a, &["response", "signature"])?).ok_or_else(|| invalid("bad signature"))?;
    let mut msg = auth_data.clone();
    msg.extend_from_slice(&crypto::sha256(&client_data));
    if !verify_sig(&row.get::<Vec<u8>, _>(2), &msg, &sig) {
        return Ok(None);
    }
    // 签名计数器：认证器支持计数时必须递增（防克隆）
    let stored: i64 = row.get(3);
    let now = i64::from(ad.sign_count);
    if (now != 0 || stored != 0) && now <= stored {
        risk::security_event(ctx, uid, "webauthn_counter_regression", json!({ "credential": cid })).await?;
        ctx.sql.commit().await?;
        return Ok(None);
    }
    let mut conn = ctx.sql.conn().await?;
    sqlx::query("UPDATE webauthn_credentials SET sign_count = $2, last_used_at = now() WHERE id = $1")
        .bind(cid)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    Ok(Some(uid))
}

fn ticket_key(ticket: &str) -> String {
    format!("wa:auth:{}", crypto::b64(&crypto::sha256(ticket.as_bytes())))
}

/// 登录第二步的断言选项（票据须有效）
pub async fn ticket_options(ctx: &Ctx, ticket: &str) -> PResult<Value> {
    let uid = two_factor::peek_ticket(ctx, ticket).await?;
    let rp = rp(ctx)?;
    let ids: Vec<Vec<u8>> = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query_scalar("SELECT credential_id FROM webauthn_credentials WHERE user_id = $1")
            .bind(uid)
            .fetch_all(&mut *conn)
            .await?
    };
    if ids.is_empty() {
        return Err(PluginError::not_found("no security keys"));
    }
    let challenge = crypto::b64(&ctx.random.bytes(32));
    redis_put(ctx, &ticket_key(ticket), &json!({ "challenge": challenge })).await?;
    Ok(json!({ "public_key": {
        "challenge": challenge, "rpId": rp.id, "timeout": 120_000, "userVerification": "preferred",
        "allowCredentials": ids.iter().map(|id| json!({ "type": "public-key", "id": crypto::b64(id) })).collect::<Vec<_>>(),
    }}))
}

/// 第二因素：校验票据对应的断言
pub async fn verify_second_factor(ctx: &Ctx, uid: i64, ticket: &str, a: &Value) -> PResult<bool> {
    let Some(st) = redis_take(ctx, &ticket_key(ticket)).await? else {
        return Ok(false);
    };
    Ok(verify_assertion(ctx, st["challenge"].as_str().unwrap_or(""), a, Some(uid), false)
        .await?
        .is_some())
}

/// 免密码登录的断言选项（可发现凭据，不列出凭据 ID）
pub async fn passwordless_options(ctx: &Ctx) -> PResult<Value> {
    let rp = rp(ctx)?;
    let challenge = crypto::b64(&ctx.random.bytes(32));
    let ceremony = crypto::b64(&ctx.random.bytes(16));
    redis_put(
        ctx,
        &format!("wa:disc:{ceremony}"),
        &json!({ "challenge": challenge }),
    )
    .await?;
    Ok(json!({ "ceremony_id": ceremony, "public_key": {
        "challenge": challenge, "rpId": rp.id, "timeout": 120_000, "userVerification": "required", "allowCredentials": [],
    }}))
}

/// 免密码登录：要求用户验证（UV），视为通过两步验证
pub async fn passwordless_login(
    id: &Identity,
    ctx: &Ctx,
    ceremony: &str,
    a: &Value,
    device_cookie: Option<&str>,
) -> PResult<NewSession> {
    let fail = || PluginError::new("identity.webauthn_invalid", "the security key was not accepted").with_status(401);
    let st = redis_take(ctx, &format!("wa:disc:{ceremony}"))
        .await?
        .ok_or_else(fail)?;
    let Some(uid) = verify_assertion(ctx, st["challenge"].as_str().unwrap_or(""), a, None, true).await? else {
        return Err(fail());
    };
    let target = two_factor::login_target(ctx, uid).await?;
    let pol = Policy::load(ctx).await;
    let mut s = auth::finish_login(id, ctx, &pol, target, "webauthn", true, true, device_cookie).await?;
    s.second_factor = Some("webauthn");
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn es256_roundtrip() {
        use p256::ecdsa::{SigningKey, signature::Signer};
        let sk = SigningKey::from_slice(&[7u8; 32]).unwrap_or_else(|_| unreachable!());
        let pt = sk.verifying_key().to_encoded_point(false);
        // COSE: {1: 2, 3: -7, -1: 1, -2: x, -3: y}
        let mut key = vec![0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
        key.extend_from_slice(pt.x().map(|x| x.as_slice()).unwrap_or_default());
        key.extend_from_slice(&[0x22, 0x58, 0x20]);
        key.extend_from_slice(pt.y().map(|y| y.as_slice()).unwrap_or_default());
        assert_eq!(cose_alg(&key), Some(ALG_ES256));
        let sig: p256::ecdsa::Signature = sk.sign(b"hello");
        assert!(verify_sig(&key, b"hello", sig.to_der().as_bytes()));
        assert!(!verify_sig(&key, b"hellO", sig.to_der().as_bytes()));
    }

    #[test]
    fn auth_data_parsing() {
        let mut d = vec![9u8; 32];
        d.push(FLAG_UP | FLAG_UV);
        d.extend_from_slice(&5u32.to_be_bytes());
        let ad = parse_auth_data(&d).unwrap_or_else(|| unreachable!());
        assert_eq!((ad.flags & FLAG_UV, ad.sign_count, ad.attested.is_none()), (FLAG_UV, 5, true));
        assert!(parse_auth_data(&d[..36]).is_none());
    }
}

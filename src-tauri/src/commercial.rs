use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const SERVICE: &str = "SpacePilot";
const SESSION_ACCOUNT: &str = "desktop-session";
const DEVICE_ACCOUNT: &str = "device-identifier";
const DEFAULT_BACKEND: &str = "https://api.spacepilot.app";
const PUBLIC_KEY: &str = include_str!("entitlement_public_key.txt");

#[derive(Clone, Serialize, Deserialize)]
pub struct SecureSession {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    entitlement_assertion: Option<String>,
}

#[derive(Deserialize)]
struct EntitlementPayload {
    version: u8,
    device_public_id: String,
    plan: String,
    capabilities: Value,
    expires_at: u64,
    grace_until: u64,
}

#[derive(Serialize)]
pub struct VerifiedEntitlement {
    plan: String,
    capabilities: Value,
    state: String,
    expires_at: u64,
    grace_until: u64,
}

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account)
        .map_err(|_| "Secure Windows credential storage is unavailable.".into())
}

fn load_session_inner() -> Result<Option<SecureSession>, String> {
    match entry(SESSION_ACCOUNT)?.get_password() {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|_| "The secure session is corrupted; sign in again.".into()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => {
            Err("The secure session could not be read from Windows Credential Manager.".into())
        }
    }
}

fn store_session(value: &SecureSession) -> Result<(), String> {
    let raw =
        serde_json::to_string(value).map_err(|_| "The secure session could not be encoded.")?;
    entry(SESSION_ACCOUNT)?
        .set_password(&raw)
        .map_err(|_| "The secure session could not be saved to Windows Credential Manager.".into())
}

#[tauri::command]
pub fn secure_session_status() -> Result<bool, String> {
    Ok(load_session_inner()?.is_some())
}

#[tauri::command]
pub fn secure_logout_local() -> Result<(), String> {
    match entry(SESSION_ACCOUNT)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("The secure session could not be removed.".into()),
    }
}

#[tauri::command]
pub fn stable_device_id() -> Result<String, String> {
    let credential = entry(DEVICE_ACCOUNT)?;
    match credential.get_password() {
        Ok(value) if value.len() >= 20 => Ok(value),
        Ok(_) | Err(keyring::Error::NoEntry) => {
            let value = format!("sp-{}", uuid::Uuid::new_v4());
            credential
                .set_password(&value)
                .map_err(|_| "The device identity could not be secured.".to_string())?;
            Ok(value)
        }
        Err(_) => Err("The device identity could not be read.".into()),
    }
}

fn backend_url() -> String {
    if cfg!(debug_assertions) {
        match std::env::var("SPACEPILOT_BACKEND_URL") {
            Ok(value)
                if value.starts_with("https://")
                    || value.starts_with("http://127.0.0.1:")
                    || value.starts_with("http://localhost:") =>
            {
                value.trim_end_matches('/').into()
            }
            _ => "http://127.0.0.1:8000".into(),
        }
    } else {
        DEFAULT_BACKEND.into()
    }
}

async fn send(
    method: &str,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> Result<(u16, Value), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(25))
        .build()
        .map_err(|_| "Backend client initialization failed.")?;
    let url = format!("{}{}", backend_url(), path);
    let mut request = match method {
        "GET" => client.get(url),
        "DELETE" => client.delete(url),
        _ => client.post(url),
    };
    if let Some(value) = token {
        request = request.bearer_auth(value);
    }
    if let Some(value) = body {
        request = request.json(&value);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "BACKEND_UNAVAILABLE".to_string())?;
    let status = response.status().as_u16();
    let payload = response.json().await.unwrap_or_else(|_| json!({}));
    Ok((status, payload))
}

fn backend_error(status: u16, payload: &Value) -> String {
    payload
        .pointer("/detail/code")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("BACKEND_HTTP_{status}"))
}

#[tauri::command]
pub async fn account_auth(
    action: String,
    email: String,
    password: String,
) -> Result<Value, String> {
    if action != "login" && action != "signup" {
        return Err("INVALID_REQUEST".into());
    }
    let (status, payload) = send(
        "POST",
        &format!("/v1/auth/{action}"),
        Some(json!({"email":email,"password":password})),
        None,
    )
    .await?;
    if status >= 400 {
        return Err(backend_error(status, &payload));
    }
    let session = SecureSession {
        access_token: payload["access_token"]
            .as_str()
            .ok_or("AUTH_INVALID")?
            .into(),
        refresh_token: payload["refresh_token"]
            .as_str()
            .ok_or("AUTH_INVALID")?
            .into(),
        entitlement_assertion: None,
    };
    store_session(&session)?;
    Ok(json!({"signed_in":true}))
}

#[tauri::command]
pub async fn backend_request(
    method: String,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    if !path.starts_with("/v1/") || path.contains("://") {
        return Err("INVALID_REQUEST".into());
    }
    let mut session = load_session_inner()?.ok_or("AUTH_REQUIRED")?;
    let (mut status, mut payload) =
        send(&method, &path, body.clone(), Some(&session.access_token)).await?;
    if status == 401
        && payload.pointer("/detail/code").and_then(Value::as_str) == Some("AUTH_EXPIRED")
    {
        refresh_session().await?;
        session = load_session_inner()?.ok_or("AUTH_REQUIRED")?;
        (status, payload) = send(&method, &path, body, Some(&session.access_token)).await?;
    }
    if status >= 400 {
        let error = backend_error(status, &payload);
        if error == "DEVICE_REVOKED" || error == "AUTH_REVOKED" {
            let _ = secure_logout_local();
        }
        return Err(error);
    }
    Ok(payload)
}

#[tauri::command]
pub async fn refresh_session() -> Result<(), String> {
    let old = load_session_inner()?.ok_or("AUTH_REQUIRED")?;
    let (status, payload) = send(
        "POST",
        "/v1/auth/refresh",
        Some(json!({"refresh_token":old.refresh_token})),
        None,
    )
    .await?;
    if status >= 400 {
        secure_logout_local()?;
        return Err(backend_error(status, &payload));
    }
    store_session(&SecureSession {
        access_token: payload["access_token"]
            .as_str()
            .ok_or("AUTH_INVALID")?
            .into(),
        refresh_token: payload["refresh_token"]
            .as_str()
            .ok_or("AUTH_INVALID")?
            .into(),
        entitlement_assertion: old.entitlement_assertion,
    })
}

#[tauri::command]
pub async fn account_logout() -> Result<(), String> {
    if let Some(session) = load_session_inner()? {
        let _ = send(
            "POST",
            "/v1/auth/logout",
            Some(json!({"refresh_token":session.refresh_token})),
            None,
        )
        .await;
    }
    secure_logout_local()
}

pub fn verify_assertion(assertion: &str, device_id: &str) -> Result<VerifiedEntitlement, String> {
    let pieces: Vec<&str> = assertion.split('.').collect();
    if pieces.len() != 3 {
        return Err("ENTITLEMENT_INVALID".into());
    }
    let public: [u8; 32] = URL_SAFE_NO_PAD
        .decode(PUBLIC_KEY.trim())
        .map_err(|_| "ENTITLEMENT_KEY_INVALID")?
        .try_into()
        .map_err(|_| "ENTITLEMENT_KEY_INVALID")?;
    let signature = Signature::from_slice(
        &URL_SAFE_NO_PAD
            .decode(pieces[2])
            .map_err(|_| "ENTITLEMENT_INVALID")?,
    )
    .map_err(|_| "ENTITLEMENT_INVALID")?;
    VerifyingKey::from_bytes(&public)
        .map_err(|_| "ENTITLEMENT_KEY_INVALID")?
        .verify(
            format!("{}.{}", pieces[0], pieces[1]).as_bytes(),
            &signature,
        )
        .map_err(|_| "ENTITLEMENT_INVALID")?;
    let payload: EntitlementPayload = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(pieces[1])
            .map_err(|_| "ENTITLEMENT_INVALID")?,
    )
    .map_err(|_| "ENTITLEMENT_INVALID")?;
    if payload.version != 1 || payload.device_public_id != device_id {
        return Err("ENTITLEMENT_INVALID".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "CLOCK_INVALID")?
        .as_secs();
    let state = if now <= payload.expires_at {
        "ONLINE_VERIFIED"
    } else if now <= payload.grace_until {
        "OFFLINE_GRACE"
    } else {
        return Err("ENTITLEMENT_EXPIRED".into());
    };
    Ok(VerifiedEntitlement {
        plan: payload.plan,
        capabilities: payload.capabilities,
        state: state.into(),
        expires_at: payload.expires_at,
        grace_until: payload.grace_until,
    })
}

#[tauri::command]
pub fn verify_cached_entitlement(
    assertion: String,
    device_id: String,
) -> Result<VerifiedEntitlement, String> {
    verify_assertion(&assertion, &device_id)
}

#[tauri::command]
pub fn cache_entitlement(assertion: String) -> Result<(), String> {
    let mut session = load_session_inner()?.ok_or("AUTH_REQUIRED")?;
    session.entitlement_assertion = Some(assertion);
    store_session(&session)
}

pub fn copilot_query_blocking(question: &str, context: &str) -> Result<Value, String> {
    let session = load_session_inner()?.ok_or("AUTH_REQUIRED")?;
    let device_id = stable_device_id()?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "Backend client initialization failed.")?;
    let response = client
        .post(format!("{}/v1/copilot/query", backend_url()))
        .bearer_auth(session.access_token)
        .json(&json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "device_id": device_id,
            "question": question,
            "context": serde_json::from_str::<Value>(context).map_err(|_| "AI_CONTEXT_INVALID")?
        }))
        .send()
        .map_err(|_| "BACKEND_UNAVAILABLE".to_string())?;
    let status = response.status().as_u16();
    let payload = response.json().unwrap_or_else(|_| json!({}));
    if status >= 400 {
        return Err(backend_error(status, &payload));
    }
    Ok(payload)
}

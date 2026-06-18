use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use rand::Rng;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};
use tauri::{Emitter, Manager};
use tokio::time;

use crate::vault::{default_settings, read_store, to_snapshot, write_store};
use crate::{
    error_string, escape_pointer, node_platform, now, random_id, read_json_file, write_json_file,
    AppState,
};

fn sync_path(state: &AppState) -> PathBuf {
    state.data_dir.join("sync.json")
}

pub(crate) fn sync_config(state: &AppState) -> Result<Value, String> {
    Ok(sync_public_config(&read_sync_store(state)?))
}

fn default_sync_store() -> Value {
    json!({
        "format": "shelldesk-sync-settings",
        "version": 1,
        "updatedAt": now(),
        "config": {
            "enabled": false,
            "provider": "webdav",
            "webdavUrl": "",
            "webdavUsername": "",
            "webdavRemotePath": "/ShellDesk/shelldesk-sync.json",
            "ignoreCertificateErrors": false,
            "intervalMinutes": 15,
            "syncOnStartup": true,
            "lastSyncAt": "",
            "lastSyncStatus": "idle",
            "lastSyncMessage": "尚未同步",
            "lastConflictCount": 0
        },
        "secrets": {
            "webdavPassword": "",
            "syncPassphrase": ""
        },
        "state": {
            "deviceId": random_id("device"),
            "lastRecords": {},
            "lastTombstones": {},
            "lastSyncAt": "",
            "lastRemoteEtag": ""
        }
    })
}

fn read_sync_store(state: &AppState) -> Result<Value, String> {
    let raw = read_json_file(&sync_path(state), json!({}))?;
    if raw
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "shelldesk-sync-settings")
    {
        if raw
            .get("protected")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return normalize_electron_protected_sync_store(raw);
        }
        return Ok(normalize_sync_store(raw));
    }
    Ok(normalize_legacy_sync_store(raw))
}

fn normalize_electron_protected_sync_store(mut raw: Value) -> Result<Value, String> {
    let ciphertext = raw
        .get("ciphertext")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "同步设置密文缺失，请重新保存同步设置。".to_string())?;
    let plaintext = decrypt_electron_safe_storage(ciphertext)?;
    let secrets: Value = serde_json::from_str(&plaintext)
        .map_err(|_| "同步设置密文内容无效，请重新保存同步设置。".to_string())?;
    raw["protected"] = json!(false);
    raw["secrets"] = normalize_sync_secrets(&secrets);
    if let Some(object) = raw.as_object_mut() {
        object.remove("ciphertext");
    }
    Ok(normalize_sync_store(raw))
}

fn normalize_sync_secrets(raw: &Value) -> Value {
    json!({
        "webdavPassword": raw.get("webdavPassword").and_then(Value::as_str).unwrap_or(""),
        "syncPassphrase": raw.get("syncPassphrase").and_then(Value::as_str).unwrap_or("")
    })
}

#[cfg(windows)]
fn decrypt_electron_safe_storage(ciphertext: &str) -> Result<String, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let mut encrypted = base64::engine::general_purpose::STANDARD
        .decode(ciphertext)
        .map_err(|_| "同步设置密文格式无效，请重新保存同步设置。".to_string())?;
    if encrypted.is_empty() {
        return Err("同步设置密文为空，请重新保存同步设置。".to_string());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("无法解密 Electron safeStorage 同步设置，请在同一 Windows 用户下运行或重新保存同步设置。".to_string());
    }
    let bytes = if output.pbData.is_null() || output.cbData == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() }
    };
    if !output.pbData.is_null() {
        unsafe {
            let _ = LocalFree(output.pbData as _);
        }
    }
    String::from_utf8(bytes).map_err(|_| "同步设置密文内容不是有效 UTF-8。".to_string())
}

#[cfg(not(windows))]
fn decrypt_electron_safe_storage(_ciphertext: &str) -> Result<String, String> {
    Err("当前平台无法直接解密 Electron safeStorage 同步设置；请重新保存同步设置。".to_string())
}

fn normalize_legacy_sync_store(raw: Value) -> Value {
    let defaults = default_sync_store();
    let config = normalize_sync_config(&raw, defaults.get("config").unwrap());
    let state = json!({
        "deviceId": raw.get("deviceId").and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or_else(|| {
            defaults.pointer("/state/deviceId").and_then(Value::as_str).unwrap_or("tauri-local")
        }),
        "lastRecords": raw.get("lastRecords").cloned().unwrap_or_else(|| json!({})),
        "lastTombstones": raw.get("lastTombstones").cloned().unwrap_or_else(|| json!({})),
        "lastSyncAt": raw.get("lastSyncAt").and_then(Value::as_str).unwrap_or(""),
        "lastRemoteEtag": raw.get("lastRemoteEtag").and_then(Value::as_str).unwrap_or("")
    });
    json!({
        "format": "shelldesk-sync-settings",
        "version": 1,
        "updatedAt": now(),
        "config": config,
        "secrets": {
            "webdavPassword": raw.get("webdavPassword").and_then(Value::as_str).unwrap_or(""),
            "syncPassphrase": raw.get("syncPassphrase").and_then(Value::as_str).unwrap_or("")
        },
        "state": state
    })
}

fn normalize_sync_store(raw: Value) -> Value {
    let defaults = default_sync_store();
    let config = normalize_sync_config(
        raw.get("config").unwrap_or(&Value::Null),
        defaults.get("config").unwrap(),
    );
    let state_defaults = defaults.get("state").unwrap();
    let state = json!({
        "deviceId": raw.pointer("/state/deviceId").and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or_else(|| state_defaults.get("deviceId").and_then(Value::as_str).unwrap_or("tauri-local")),
        "lastRecords": raw.pointer("/state/lastRecords").cloned().unwrap_or_else(|| json!({})),
        "lastTombstones": raw.pointer("/state/lastTombstones").cloned().unwrap_or_else(|| json!({})),
        "lastSyncAt": raw.pointer("/state/lastSyncAt").and_then(Value::as_str).unwrap_or(""),
        "lastRemoteEtag": raw.pointer("/state/lastRemoteEtag").and_then(Value::as_str).unwrap_or("")
    });
    json!({
        "format": "shelldesk-sync-settings",
        "version": 1,
        "updatedAt": raw.get("updatedAt").and_then(Value::as_str).unwrap_or(""),
        "config": config,
        "secrets": {
            "webdavPassword": raw.pointer("/secrets/webdavPassword").and_then(Value::as_str).unwrap_or(""),
            "syncPassphrase": raw.pointer("/secrets/syncPassphrase").and_then(Value::as_str).unwrap_or("")
        },
        "state": state
    })
}

fn normalize_sync_config(raw: &Value, fallback: &Value) -> Value {
    let enabled = raw
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            fallback
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
    let interval = raw
        .get("intervalMinutes")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| {
            fallback
                .get("intervalMinutes")
                .and_then(Value::as_i64)
                .unwrap_or(15)
        })
        .clamp(5, 1440);
    json!({
        "enabled": enabled,
        "provider": "webdav",
        "webdavUrl": normalize_webdav_url(raw.get("webdavUrl").and_then(Value::as_str).or_else(|| fallback.get("webdavUrl").and_then(Value::as_str)).unwrap_or(""), enabled).unwrap_or_default(),
        "webdavUsername": raw.get("webdavUsername").and_then(Value::as_str).or_else(|| fallback.get("webdavUsername").and_then(Value::as_str)).unwrap_or(""),
        "webdavRemotePath": normalize_webdav_remote_path(raw.get("webdavRemotePath").and_then(Value::as_str).or_else(|| fallback.get("webdavRemotePath").and_then(Value::as_str)).unwrap_or("/ShellDesk/shelldesk-sync.json")).unwrap_or_else(|_| "/ShellDesk/shelldesk-sync.json".to_string()),
        "ignoreCertificateErrors": raw.get("ignoreCertificateErrors").and_then(Value::as_bool).unwrap_or_else(|| fallback.get("ignoreCertificateErrors").and_then(Value::as_bool).unwrap_or(false)),
        "intervalMinutes": interval,
        "syncOnStartup": raw.get("syncOnStartup").and_then(Value::as_bool).unwrap_or_else(|| fallback.get("syncOnStartup").and_then(Value::as_bool).unwrap_or(true)),
        "lastSyncAt": raw.get("lastSyncAt").and_then(Value::as_str).or_else(|| fallback.get("lastSyncAt").and_then(Value::as_str)).unwrap_or(""),
        "lastSyncStatus": normalize_sync_status(raw.get("lastSyncStatus").and_then(Value::as_str).or_else(|| fallback.get("lastSyncStatus").and_then(Value::as_str)).unwrap_or("idle")),
        "lastSyncMessage": raw.get("lastSyncMessage").and_then(Value::as_str).or_else(|| fallback.get("lastSyncMessage").and_then(Value::as_str)).unwrap_or("尚未同步"),
        "lastConflictCount": raw.get("lastConflictCount").and_then(Value::as_i64).or_else(|| fallback.get("lastConflictCount").and_then(Value::as_i64)).unwrap_or(0).clamp(0, 10000)
    })
}

fn normalize_sync_status(value: &str) -> &str {
    match value {
        "success" | "warning" | "error" => value,
        _ => "idle",
    }
}

fn normalize_webdav_url(value: &str, required: bool) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        if required {
            return Err("请输入 WebDAV 地址。".to_string());
        }
        return Ok(String::new());
    }
    let mut parsed = reqwest::Url::parse(trimmed).map_err(|_| "WebDAV 地址无效。".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("WebDAV 地址只支持 http 或 https。".to_string());
    }
    parsed.set_fragment(None);
    Ok(parsed.to_string())
}

fn write_sync_store(state: &AppState, mut store: Value) -> Result<Value, String> {
    store["updatedAt"] = json!(now());
    write_json_file(&sync_path(state), &store)?;
    Ok(store)
}

fn sync_public_config(store: &Value) -> Value {
    let config = store.get("config").cloned().unwrap_or_else(|| json!({}));
    let secrets = store.get("secrets").cloned().unwrap_or_else(|| json!({}));
    let state = store.get("state").cloned().unwrap_or_else(|| json!({}));
    json!({
        "enabled": config.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        "provider": "webdav",
        "webdavUrl": config.get("webdavUrl").and_then(Value::as_str).unwrap_or(""),
        "webdavUsername": config.get("webdavUsername").and_then(Value::as_str).unwrap_or(""),
        "webdavRemotePath": config.get("webdavRemotePath").and_then(Value::as_str).unwrap_or("/ShellDesk/shelldesk-sync.json"),
        "ignoreCertificateErrors": config.get("ignoreCertificateErrors").and_then(Value::as_bool).unwrap_or(false),
        "intervalMinutes": config.get("intervalMinutes").and_then(Value::as_i64).unwrap_or(15),
        "syncOnStartup": config.get("syncOnStartup").and_then(Value::as_bool).unwrap_or(true),
        "lastSyncAt": config.get("lastSyncAt").and_then(Value::as_str).unwrap_or(""),
        "lastSyncStatus": config.get("lastSyncStatus").and_then(Value::as_str).unwrap_or("idle"),
        "lastSyncMessage": config.get("lastSyncMessage").and_then(Value::as_str).unwrap_or(""),
        "lastConflictCount": config.get("lastConflictCount").and_then(Value::as_i64).unwrap_or(0),
        "deviceId": state.get("deviceId").and_then(Value::as_str).unwrap_or("tauri-local"),
        "hasWebDavPassword": secrets.get("webdavPassword").and_then(Value::as_str).is_some_and(|value| !value.is_empty()),
        "hasSyncPassphrase": secrets.get("syncPassphrase").and_then(Value::as_str).is_some_and(|value| !value.is_empty())
    })
}

fn read_incoming_secret(value: Option<&Value>, previous: &str) -> String {
    match value.and_then(Value::as_str) {
        Some(text) if !text.is_empty() && text != "••••••••" => text.to_string(),
        _ => previous.to_string(),
    }
}

pub(crate) fn save_sync_config(state: &AppState, incoming: Value) -> Result<Value, String> {
    let current = read_sync_store(state)?;
    let fallback_config = current.get("config").cloned().unwrap_or_else(|| json!({}));
    let incoming_object = incoming
        .as_object()
        .ok_or_else(|| "同步设置无效。".to_string())?;
    let next_config = normalize_sync_config(&incoming, &fallback_config);
    let current_secrets = current.get("secrets").cloned().unwrap_or_else(|| json!({}));
    let next_secrets = json!({
        "webdavPassword": read_incoming_secret(incoming_object.get("webdavPassword"), current_secrets.get("webdavPassword").and_then(Value::as_str).unwrap_or("")),
        "syncPassphrase": read_incoming_secret(incoming_object.get("syncPassphrase"), current_secrets.get("syncPassphrase").and_then(Value::as_str).unwrap_or(""))
    });
    if next_config
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        ensure_operational_sync_settings(&next_config, &next_secrets, true)?;
    }
    let next = json!({
        "format": "shelldesk-sync-settings",
        "version": 1,
        "config": next_config,
        "secrets": next_secrets,
        "state": current.get("state").cloned().unwrap_or_else(|| default_sync_store().get("state").cloned().unwrap_or_else(|| json!({})))
    });
    let saved = write_sync_store(state, next)?;
    Ok(sync_public_config(&saved))
}

fn next_sync_schedule_generation(state: &AppState) -> u64 {
    let mut generation = state.sync_schedule_generation.lock().unwrap();
    *generation = generation.saturating_add(1);
    *generation
}

fn current_sync_schedule_generation(state: &AppState) -> u64 {
    *state.sync_schedule_generation.lock().unwrap()
}

pub(crate) fn reload_sync_schedule(state: &AppState, app: &tauri::AppHandle) {
    let generation = next_sync_schedule_generation(state);
    let store = match read_sync_store(state) {
        Ok(store) => store,
        Err(_) => return,
    };
    let config = store.get("config").cloned().unwrap_or_else(|| json!({}));
    if !config
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return;
    }
    let interval_minutes = config
        .get("intervalMinutes")
        .and_then(Value::as_i64)
        .unwrap_or(15)
        .clamp(5, 1440) as u64;
    let sync_on_startup = config
        .get("syncOnStartup")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let state = state.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if sync_on_startup {
            time::sleep(Duration::from_secs(12)).await;
            if current_sync_schedule_generation(&state) != generation {
                return;
            }
            run_scheduled_webdav_sync(&state, &app).await;
        }
        loop {
            time::sleep(Duration::from_secs(interval_minutes * 60)).await;
            if current_sync_schedule_generation(&state) != generation {
                return;
            }
            run_scheduled_webdav_sync(&state, &app).await;
        }
    });
}

async fn run_scheduled_webdav_sync(state: &AppState, app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = run_webdav_sync(state, &window, vec![]).await {
        let _ = update_sync_status(state, "error", &error);
        let result = sync_config(state).map(|config| {
            json!({
                "ok": false,
                "needsResolution": false,
                "needsEmptyVaultResolution": false,
                "needsShrinkConfirmation": false,
                "resolution": "",
                "emptyVaultResolution": "",
                "shrinkResolution": "",
                "syncedAt": "",
                "uploaded": 0,
                "downloaded": 0,
                "deleted": 0,
                "conflictCount": 0,
                "conflicts": [],
                "conflictSummary": [],
                "summary": {
                    "localRecords": 0,
                    "remoteRecords": 0,
                    "mergedRecords": 0,
                    "tombstones": 0,
                    "uploaded": 0,
                    "downloaded": 0,
                    "deleted": 0,
                    "conflictCount": 0,
                    "conflictsByType": [],
                    "recordsByType": {}
                },
                "emptyVaultSummary": null,
                "shrinkSummary": null,
                "snapshot": null,
                "config": config,
                "message": error
            })
        });
        if let Ok(result) = result {
            let _ = window.emit("sync:changed", result);
        }
    }
}

pub(crate) async fn test_webdav(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    let store = operational_sync_store(state, args.first().cloned(), false)?;
    let config = store.get("config").cloned().unwrap_or_else(|| json!({}));
    let secrets = store.get("secrets").cloned().unwrap_or_else(|| json!({}));
    ensure_webdav_directories(&config, &secrets).await?;
    let test_path = webdav_test_path(&config);
    let content = format!("ShellDesk WebDAV test {}", now());
    let put = webdav_request(
        &config,
        &secrets,
        "PUT",
        &test_path,
        Some(content.clone()),
        Some("text/plain; charset=utf-8"),
        &[],
    )
    .await?;
    if !matches!(put.status().as_u16(), 200 | 201 | 204) {
        return Err(webdav_response_error(put, "写入 WebDAV 测试文件").await);
    }
    let get = webdav_request(&config, &secrets, "GET", &test_path, None, None, &[]).await?;
    if !get.status().is_success() {
        return Err(webdav_response_error(get, "读取 WebDAV 测试文件").await);
    }
    let read_back = get.text().await.map_err(error_string)?;
    if read_back != content {
        return Err("WebDAV 测试文件读写内容不一致。".to_string());
    }
    let delete = webdav_request(&config, &secrets, "DELETE", &test_path, None, None, &[]).await?;
    let cleanup_warning = if delete.status().is_success() || delete.status().as_u16() == 404 {
        String::new()
    } else {
        format!("读写测试通过，但临时测试文件删除失败：{}", delete.status())
    };
    let message = if cleanup_warning.is_empty() {
        "WebDAV 连接测试通过，远程目录具备读写权限。".to_string()
    } else {
        cleanup_warning
    };
    update_sync_status(
        state,
        if message.starts_with("读写测试通过") {
            "warning"
        } else {
            "success"
        },
        &message,
    )?;
    Ok(json!({ "ok": true, "checkedAt": now(), "message": message }))
}

pub(crate) async fn run_webdav_sync<R, W>(
    state: &AppState,
    window: &W,
    args: Vec<Value>,
) -> Result<Value, String>
where
    R: tauri::Runtime,
    W: Emitter<R>,
{
    let incoming = args.first().cloned();
    let mut store = operational_sync_store(state, incoming.clone(), true)?;
    let config = store.get("config").cloned().unwrap_or_else(|| json!({}));
    let secrets = store.get("secrets").cloned().unwrap_or_else(|| json!({}));
    ensure_webdav_directories(&config, &secrets).await?;
    let conflict_resolution = read_resolution(
        incoming.as_ref(),
        "conflictResolution",
        &["local", "remote"],
    );
    let empty_vault_resolution = read_resolution(
        incoming.as_ref(),
        "emptyVaultResolution",
        &["restoreRemote", "keepEmpty"],
    );
    let shrink_resolution = read_resolution(incoming.as_ref(), "shrinkResolution", &["allow"]);
    let max_precondition_retries = 1;
    let max_local_refreshes = 2;
    let mut precondition_retries = 0;
    let mut local_refreshes = 0;
    let mut remote_override: Option<Value> = None;

    loop {
        if precondition_retries > max_precondition_retries || local_refreshes > max_local_refreshes
        {
            return Err("同步未完成。".to_string());
        }

        let now_value = now();
        let local = create_local_sync_inputs(
            state,
            store.get("state").unwrap_or(&Value::Null),
            &now_value,
        )?;
        let remote = if let Some(remote) = remote_override.take() {
            remote
        } else {
            read_remote_sync_document(&config, &secrets).await?
        };
        let mut effective_local_records = local
            .get("localRecords")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut effective_local_tombstones = local
            .get("localTombstones")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let local_count = count_content_records(&effective_local_records);
        let remote_document = remote
            .get("document")
            .cloned()
            .unwrap_or_else(create_empty_remote_document);
        let remote_count =
            count_content_records(remote_document.get("records").unwrap_or(&Value::Null));

        if empty_vault_resolution.is_empty() && local_count == 0 && remote_count > 0 {
            let result = pending_empty_vault_result(state, &mut store, &local, &remote_document)?;
            let _ = window.emit("sync:changed", result.clone());
            return Ok(result);
        }

        if empty_vault_resolution == "restoreRemote" && local_count == 0 && remote_count > 0 {
            if remote_document.pointer("/records/settings:app").is_some() {
                if let Some(object) = effective_local_records.as_object_mut() {
                    object.remove("settings:app");
                }
            }
            effective_local_tombstones = json!({});
        } else if empty_vault_resolution == "keepEmpty" && local_count == 0 && remote_count > 0 {
            effective_local_tombstones = merge_objects(
                effective_local_tombstones,
                tombstones_for_records(
                    remote_document.get("records").unwrap_or(&Value::Null),
                    store.get("state").unwrap_or(&Value::Null),
                    &now_value,
                ),
            );
        }

        let merged = merge_sync_documents(
            &remote_document,
            &effective_local_records,
            &effective_local_tombstones,
            store.get("state").unwrap_or(&Value::Null),
            &now_value,
            &conflict_resolution,
        );
        let merged_document = merged
            .get("document")
            .cloned()
            .unwrap_or_else(create_empty_remote_document);
        let conflicts = merged
            .get("conflicts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if !conflicts.is_empty() && conflict_resolution.is_empty() {
            let result =
                pending_conflict_result(state, &mut store, &local, &remote_document, &merged)?;
            let _ = window.emit("sync:changed", result.clone());
            return Ok(result);
        }

        let shrink = detect_suspicious_shrink(
            store.get("state").unwrap_or(&Value::Null),
            &effective_local_records,
            &remote_document,
            &merged_document,
        );
        if shrink.is_some() && shrink_resolution != "allow" && empty_vault_resolution != "keepEmpty"
        {
            let result = pending_shrink_result(
                state,
                &mut store,
                &local,
                &remote_document,
                &merged,
                shrink,
            )?;
            let _ = window.emit("sync:changed", result.clone());
            return Ok(result);
        }

        let write_result = write_remote_sync_document(
            &config,
            &secrets,
            &merged_document,
            remote.get("etag").and_then(Value::as_str).unwrap_or(""),
            remote
                .get("exists")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .await?;
        if write_result
            .get("preconditionFailed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            if precondition_retries < max_precondition_retries {
                precondition_retries += 1;
                continue;
            }
            return Err("远端同步文件刚刚被其他设备更新，请稍后重试。".to_string());
        }

        let latest_local = create_local_sync_inputs(
            state,
            store.get("state").unwrap_or(&Value::Null),
            &now_value,
        )?;
        if latest_local.get("footprint") != local.get("footprint") {
            if local_refreshes >= max_local_refreshes {
                return Err("本机数据在同步期间持续变化，请稍后重试。".to_string());
            }
            local_refreshes += 1;
            remote_override = Some(json!({
                "document": merged_document,
                "etag": write_result.get("etag").and_then(Value::as_str).unwrap_or(""),
                "exists": true
            }));
            continue;
        }

        let snapshot_value = apply_sync_document_to_vault(state, &merged_document)?;
        let synced_at = now();
        let message = if !conflicts.is_empty() && !conflict_resolution.is_empty() {
            format!(
                "同步完成，已按选择保留{}处理 {} 个冲突。",
                if conflict_resolution == "local" {
                    "本地"
                } else {
                    "云端"
                },
                conflicts.len()
            )
        } else {
            "同步完成。".to_string()
        };
        store["state"] = create_sync_state_from_document(
            &merged_document,
            store
                .pointer("/state/deviceId")
                .and_then(Value::as_str)
                .unwrap_or("tauri-local"),
            write_result
                .get("etag")
                .and_then(Value::as_str)
                .unwrap_or(""),
            &synced_at,
        );
        store["config"]["lastSyncAt"] = json!(synced_at);
        store["config"]["lastSyncStatus"] = json!("success");
        store["config"]["lastSyncMessage"] = json!(message);
        store["config"]["lastConflictCount"] = json!(0);
        let saved_store = write_sync_store(state, store)?;
        let _ = window.emit("vault:changed", json!({ "kind": "sync" }));
        let result = sync_success_result(
            &saved_store,
            &local,
            &remote_document,
            &merged,
            snapshot_value,
            &message,
            &conflict_resolution,
            &empty_vault_resolution,
            &shrink_resolution,
        );
        let _ = window.emit("sync:changed", result.clone());
        return Ok(result);
    }
}

fn operational_sync_store(
    state: &AppState,
    incoming: Option<Value>,
    require_sync_passphrase: bool,
) -> Result<Value, String> {
    let store = if let Some(incoming) = incoming {
        if incoming.is_null() {
            read_sync_store(state)?
        } else {
            let _ = save_sync_config(state, incoming)?;
            read_sync_store(state)?
        }
    } else {
        read_sync_store(state)?
    };
    ensure_operational_sync_settings(
        store.get("config").unwrap_or(&Value::Null),
        store.get("secrets").unwrap_or(&Value::Null),
        require_sync_passphrase,
    )?;
    Ok(store)
}

fn ensure_operational_sync_settings(
    config: &Value,
    secrets: &Value,
    require_sync_passphrase: bool,
) -> Result<(), String> {
    let webdav_url = config
        .get("webdavUrl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let username = config
        .get("webdavUsername")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let password = secrets
        .get("webdavPassword")
        .and_then(Value::as_str)
        .unwrap_or("");
    let passphrase = secrets
        .get("syncPassphrase")
        .and_then(Value::as_str)
        .unwrap_or("");
    if webdav_url.is_empty() {
        return Err("请先填写 WebDAV 地址。".to_string());
    }
    if username.is_empty() {
        return Err("请先填写 WebDAV 用户名。".to_string());
    }
    if password.is_empty() {
        return Err("请先填写 WebDAV 密码或应用密码。".to_string());
    }
    if require_sync_passphrase && passphrase.chars().count() < 8 {
        return Err("同步密码至少需要 8 个字符。".to_string());
    }
    Ok(())
}

async fn webdav_request(
    config: &Value,
    secrets: &Value,
    method: &str,
    remote_path: &str,
    body: Option<String>,
    content_type: Option<&str>,
    headers: &[(&str, String)],
) -> Result<reqwest::Response, String> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(
            config
                .get("ignoreCertificateErrors")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(error_string)?;
    let url = webdav_url(config, remote_path)?;
    let username = config
        .get("webdavUsername")
        .and_then(Value::as_str)
        .unwrap_or("");
    let password = secrets
        .get("webdavPassword")
        .and_then(Value::as_str)
        .unwrap_or("");
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(error_string)?;
    let mut request = client
        .request(method, url)
        .basic_auth(username, Some(password));
    if let Some(content_type) = content_type {
        request = request.header("Content-Type", content_type);
    }
    for (key, value) in headers {
        request = request.header(*key, value);
    }
    if let Some(body) = body {
        request = request.body(body);
    }
    request.send().await.map_err(error_string)
}

fn webdav_url(config: &Value, remote_path: &str) -> Result<String, String> {
    let base = config
        .get("webdavUrl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let mut parsed = reqwest::Url::parse(base).map_err(|_| "WebDAV 地址无效。".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("WebDAV 地址只支持 http 或 https。".to_string());
    }
    let mut path = parsed.path().trim_end_matches('/').to_string();
    let remote = normalize_webdav_remote_path(remote_path)?;
    path.push_str(&remote);
    parsed.set_path(&path);
    parsed.set_query(None);
    parsed.set_fragment(None);
    Ok(parsed.to_string())
}

fn normalize_webdav_remote_path(value: &str) -> Result<String, String> {
    let normalized = value.replace('\\', "/").replace("//", "/");
    let path = if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{normalized}")
    };
    let parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty()
        || parts.iter().any(|part| {
            *part == "."
                || *part == ".."
                || part.contains('\0')
                || part.contains('?')
                || part.contains('#')
        })
    {
        return Err("远程同步文件路径无效。".to_string());
    }
    Ok(format!("/{}", parts.join("/")))
}

async fn ensure_webdav_directories(config: &Value, secrets: &Value) -> Result<(), String> {
    let remote_path = config
        .get("webdavRemotePath")
        .and_then(Value::as_str)
        .unwrap_or("/ShellDesk/shelldesk-sync.json");
    let normalized = normalize_webdav_remote_path(remote_path)?;
    let parts = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() <= 1 {
        return Ok(());
    }
    let mut current = String::new();
    for part in parts.iter().take(parts.len() - 1) {
        current.push('/');
        current.push_str(part);
        let response = webdav_request(config, secrets, "MKCOL", &current, None, None, &[]).await?;
        if !matches!(
            response.status().as_u16(),
            200 | 201 | 204 | 301 | 302 | 405 | 409
        ) {
            return Err(webdav_response_error(response, "创建 WebDAV 远程目录").await);
        }
    }
    Ok(())
}

fn webdav_test_path(config: &Value) -> String {
    let remote_path = config
        .get("webdavRemotePath")
        .and_then(Value::as_str)
        .unwrap_or("/ShellDesk/shelldesk-sync.json");
    let normalized = normalize_webdav_remote_path(remote_path)
        .unwrap_or_else(|_| "/ShellDesk/shelldesk-sync.json".to_string());
    let parent = normalized
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
    format!("{parent}/.shelldesk-webdav-test-{}.txt", random_id("test"))
}

async fn webdav_response_error(response: reqwest::Response, action: &str) -> String {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let detail = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if detail.is_empty() {
        format!("{action}失败：{status}")
    } else {
        format!(
            "{action}失败：{status}：{}",
            detail.chars().take(180).collect::<String>()
        )
    }
}

fn item_identity(item: &Value) -> String {
    for key in ["id", "name", "address", "fingerprint", "scope", "url"] {
        if let Some(value) = item
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return format!("{key}:{value}");
        }
    }
    serde_json::to_string(item).unwrap_or_else(|_| random_id("item"))
}

fn read_resolution(incoming: Option<&Value>, key: &str, allowed: &[&str]) -> String {
    incoming
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .filter(|value| allowed.contains(value))
        .unwrap_or("")
        .to_string()
}

fn merge_objects(left: Value, right: Value) -> Value {
    let mut object = left.as_object().cloned().unwrap_or_default();
    if let Some(right_object) = right.as_object() {
        for (key, value) in right_object {
            object.insert(key.clone(), value.clone());
        }
    }
    Value::Object(object)
}

fn stable_json(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(stable_json).collect::<Vec<_>>().join(",")
        ),
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        stable_json(object.get(key).unwrap_or(&Value::Null))
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

fn hash_payload(payload: &Value) -> String {
    format!("{:x}", Sha256::digest(stable_json(payload).as_bytes()))
}

fn valid_datetime(value: Option<&str>, fallback: &str) -> String {
    value
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| fallback.to_string())
}

fn compare_time(left: &str, right: &str) -> i64 {
    let left = chrono::DateTime::parse_from_rfc3339(left)
        .map(|value| value.timestamp_millis())
        .unwrap_or(0);
    let right = chrono::DateTime::parse_from_rfc3339(right)
        .map(|value| value.timestamp_millis())
        .unwrap_or(0);
    left - right
}

fn public_host_payload(host: &Value) -> Value {
    let mut payload = host.as_object().cloned().unwrap_or_default();
    payload.insert("password".to_string(), json!(""));
    payload.insert("passphrase".to_string(), json!(""));
    payload.insert("rootPassword".to_string(), json!(""));
    Value::Object(payload)
}

fn public_settings_payload(settings: &Value) -> Value {
    let mut payload = settings.as_object().cloned().unwrap_or_default();
    payload.insert("aiApiKey".to_string(), json!(""));
    Value::Object(payload)
}

fn public_proxy_payload(profile: &Value) -> Value {
    let mut payload = profile.as_object().cloned().unwrap_or_default();
    let mut config = payload
        .get("config")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    config.insert("password".to_string(), json!(""));
    payload.insert("config".to_string(), Value::Object(config));
    Value::Object(payload)
}

fn create_sync_record(
    id: String,
    record_type: &str,
    payload: Value,
    updated_at: String,
    device_id: &str,
) -> Value {
    json!({
        "id": id,
        "type": record_type,
        "updatedAt": updated_at,
        "deviceId": device_id,
        "hash": hash_payload(&payload),
        "payload": payload
    })
}

fn create_records_from_vault(vault: &Value, sync_state: &Value, now_value: &str) -> Value {
    let device_id = sync_state
        .get("deviceId")
        .and_then(Value::as_str)
        .unwrap_or("tauri-local");
    let mut records = Map::new();

    for host in vault
        .get("hosts")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let id = host.get("id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let entity_id = format!("host:{id}");
        let payload = public_host_payload(host);
        records.insert(
            entity_id.clone(),
            create_sync_record(
                entity_id,
                "host",
                payload,
                valid_datetime(host.get("updatedAt").and_then(Value::as_str), now_value),
                device_id,
            ),
        );
    }

    for profile in vault
        .get("proxyProfiles")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let id = profile.get("id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let entity_id = format!("proxyProfile:{id}");
        let payload = public_proxy_payload(profile);
        records.insert(
            entity_id.clone(),
            create_sync_record(
                entity_id,
                "proxyProfile",
                payload,
                valid_datetime(profile.get("updatedAt").and_then(Value::as_str), now_value),
                device_id,
            ),
        );
    }

    for known_host in vault
        .get("knownHosts")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let id = known_host
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| item_identity(known_host));
        let entity_id = format!("knownHost:{id}");
        let payload = known_host.clone();
        let payload_hash = hash_payload(&payload);
        let previous = sync_state.pointer(&format!("/lastRecords/{}", escape_pointer(&entity_id)));
        let updated_at = if previous
            .and_then(|value| value.get("hash"))
            .and_then(Value::as_str)
            == Some(payload_hash.as_str())
        {
            previous
                .and_then(|value| value.get("updatedAt"))
                .and_then(Value::as_str)
                .or_else(|| sync_state.get("lastSyncAt").and_then(Value::as_str))
                .unwrap_or(now_value)
                .to_string()
        } else {
            now_value.to_string()
        };
        records.insert(
            entity_id.clone(),
            create_sync_record(entity_id, "knownHost", payload, updated_at, device_id),
        );
    }

    let settings = vault
        .get("settings")
        .cloned()
        .unwrap_or_else(default_settings);
    let settings_payload = public_settings_payload(&settings);
    let settings_hash = hash_payload(&settings_payload);
    let settings_previous = sync_state.pointer("/lastRecords/settings:app");
    let settings_updated_at = if settings_previous
        .and_then(|value| value.get("hash"))
        .and_then(Value::as_str)
        == Some(settings_hash.as_str())
    {
        settings_previous
            .and_then(|value| value.get("updatedAt"))
            .and_then(Value::as_str)
            .or_else(|| sync_state.get("lastSyncAt").and_then(Value::as_str))
            .unwrap_or(now_value)
            .to_string()
    } else {
        now_value.to_string()
    };
    records.insert(
        "settings:app".to_string(),
        json!({
            "id": "settings:app",
            "type": "settings",
            "updatedAt": settings_updated_at,
            "deviceId": device_id,
            "hash": settings_hash,
            "payload": settings_payload
        }),
    );

    for collection in vault
        .get("browserBookmarks")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let scope = collection
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("");
        let scope_hash = hash_payload(&json!(scope));
        for bookmark in collection
            .get("bookmarks")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new())
        {
            let bookmark_id = bookmark.get("id").and_then(Value::as_str).unwrap_or("");
            if bookmark_id.is_empty() {
                continue;
            }
            let entity_id = format!("bookmark:{}:{}", &scope_hash[..16], bookmark_id);
            let payload = json!({ "scope": scope, "bookmark": bookmark });
            records.insert(
                entity_id.clone(),
                create_sync_record(
                    entity_id,
                    "bookmark",
                    payload,
                    valid_datetime(
                        bookmark
                            .get("updatedAt")
                            .and_then(Value::as_str)
                            .or_else(|| collection.get("updatedAt").and_then(Value::as_str)),
                        now_value,
                    ),
                    device_id,
                ),
            );
        }
    }

    Value::Object(records)
}

fn create_local_tombstones(local_records: &Value, sync_state: &Value, now_value: &str) -> Value {
    let mut tombstones = Map::new();
    if let Some(previous_tombstones) = sync_state.get("lastTombstones").and_then(Value::as_object) {
        for (id, tombstone) in previous_tombstones {
            if local_records.get(id).is_some() {
                continue;
            }
            tombstones.insert(id.clone(), tombstone.clone());
        }
    }
    if let Some(previous_records) = sync_state.get("lastRecords").and_then(Value::as_object) {
        for (id, previous) in previous_records {
            if id == "settings:app" || local_records.get(id).is_some() {
                continue;
            }
            tombstones.insert(
                id.clone(),
                json!({
                    "id": id,
                    "type": previous.get("type").and_then(Value::as_str).unwrap_or("host"),
                    "deletedAt": now_value,
                    "deviceId": sync_state.get("deviceId").and_then(Value::as_str).unwrap_or("tauri-local"),
                    "hash": previous.get("hash").and_then(Value::as_str).unwrap_or("")
                }),
            );
        }
    }
    Value::Object(tombstones)
}

fn create_local_sync_inputs(
    state: &AppState,
    sync_state: &Value,
    now_value: &str,
) -> Result<Value, String> {
    let vault = read_store(state)?;
    let local_records = create_records_from_vault(&vault, sync_state, now_value);
    let local_tombstones = create_local_tombstones(&local_records, sync_state, now_value);
    Ok(json!({
        "localRecords": local_records,
        "localTombstones": local_tombstones,
        "footprint": create_sync_footprint(&local_records, &local_tombstones)
    }))
}

fn create_sync_footprint(records: &Value, tombstones: &Value) -> String {
    let record_footprint = records
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(id, record)| {
                    (
                        id.clone(),
                        json!({
                            "type": record.get("type").and_then(Value::as_str).unwrap_or(""),
                            "hash": record.get("hash").and_then(Value::as_str).unwrap_or("")
                        }),
                    )
                })
                .collect::<Map<_, _>>()
        })
        .unwrap_or_default();
    let tombstone_footprint = tombstones
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(id, tombstone)| {
                    (
                        id.clone(),
                        json!({
                            "type": tombstone.get("type").and_then(Value::as_str).unwrap_or(""),
                            "hash": tombstone.get("hash").and_then(Value::as_str).unwrap_or("")
                        }),
                    )
                })
                .collect::<Map<_, _>>()
        })
        .unwrap_or_default();
    stable_json(&json!({
        "records": Value::Object(record_footprint),
        "tombstones": Value::Object(tombstone_footprint)
    }))
}

fn synced_content_type(value: &str) -> bool {
    matches!(value, "host" | "bookmark" | "proxyProfile" | "knownHost")
}

fn count_records_by_type(records: &Value) -> Map<String, Value> {
    let mut counts: HashMap<String, i64> = HashMap::new();
    if let Some(object) = records.as_object() {
        for record in object.values() {
            let record_type = record.get("type").and_then(Value::as_str).unwrap_or("");
            if synced_content_type(record_type) {
                *counts.entry(record_type.to_string()).or_insert(0) += 1;
            }
        }
    }
    counts
        .into_iter()
        .map(|(key, count)| (key, json!(count)))
        .collect()
}

fn sum_counts(counts: &Map<String, Value>) -> i64 {
    counts.values().filter_map(Value::as_i64).sum()
}

fn count_content_records(records: &Value) -> i64 {
    sum_counts(&count_records_by_type(records))
}

fn conflict_summary(conflicts: &[Value]) -> Value {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for conflict in conflicts {
        let record_type = conflict.get("type").and_then(Value::as_str).unwrap_or("");
        if !record_type.is_empty() {
            *counts.entry(record_type.to_string()).or_insert(0) += 1;
        }
    }
    let mut items = counts
        .into_iter()
        .map(|(record_type, count)| json!({ "type": record_type, "count": count }))
        .collect::<Vec<_>>();
    items.sort_by_key(|item| {
        item.get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    Value::Array(items)
}

fn sync_summary(
    local_records: &Value,
    local_tombstones: &Value,
    remote_document: &Value,
    merged_document: &Value,
    uploaded: i64,
    downloaded: i64,
    deleted: i64,
    conflicts: &[Value],
) -> Value {
    let local_counts = count_records_by_type(local_records);
    let remote_counts =
        count_records_by_type(remote_document.get("records").unwrap_or(&Value::Null));
    let merged_counts =
        count_records_by_type(merged_document.get("records").unwrap_or(&Value::Null));
    let tombstones = merge_objects(
        merge_objects(
            remote_document
                .get("tombstones")
                .cloned()
                .unwrap_or_else(|| json!({})),
            local_tombstones.clone(),
        ),
        merged_document
            .get("tombstones")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    let tombstone_counts = count_records_by_type(&tombstones);
    json!({
        "localRecords": sum_counts(&local_counts),
        "remoteRecords": sum_counts(&remote_counts),
        "mergedRecords": sum_counts(&merged_counts),
        "tombstones": sum_counts(&tombstone_counts),
        "uploaded": uploaded,
        "downloaded": downloaded,
        "deleted": deleted,
        "conflictCount": conflicts.len(),
        "conflictsByType": conflict_summary(conflicts),
        "recordsByType": Value::Object(merged_counts)
    })
}

fn create_empty_remote_document() -> Value {
    json!({
        "format": "shelldesk-sync-webdav",
        "version": 1,
        "updatedAt": "",
        "devices": {},
        "records": {},
        "tombstones": {}
    })
}

fn sanitize_remote_document(raw: Value) -> Value {
    if raw.get("format").and_then(Value::as_str) == Some("shelldesk-sync-webdav")
        && raw.get("version").and_then(Value::as_i64) == Some(1)
    {
        let mut document = create_empty_remote_document();
        document["updatedAt"] = raw.get("updatedAt").cloned().unwrap_or_else(|| json!(""));
        document["devices"] = raw.get("devices").cloned().unwrap_or_else(|| json!({}));
        document["records"] = raw.get("records").cloned().unwrap_or_else(|| json!({}));
        document["tombstones"] = raw.get("tombstones").cloned().unwrap_or_else(|| json!({}));
        return document;
    }
    if let Some(snapshot_value) = raw.get("snapshot") {
        let mut document = create_empty_remote_document();
        let sync_state = json!({ "deviceId": "remote", "lastRecords": {}, "lastTombstones": {} });
        document["updatedAt"] = raw
            .get("updatedAt")
            .cloned()
            .unwrap_or_else(|| json!(now()));
        document["records"] = create_records_from_vault(snapshot_value, &sync_state, &now());
        return document;
    }
    let mut document = create_empty_remote_document();
    let sync_state = json!({ "deviceId": "remote", "lastRecords": {}, "lastTombstones": {} });
    document["updatedAt"] = json!(now());
    document["records"] = create_records_from_vault(&raw, &sync_state, &now());
    document
}

fn derive_sync_key(passphrase: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, iterations, &mut key);
    key
}

fn encrypt_remote_document(document: &Value, passphrase: &str) -> Result<Value, String> {
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    rand::thread_rng().fill(&mut salt);
    rand::thread_rng().fill(&mut iv);
    let key = derive_sync_key(passphrase, &salt, 210_000);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(error_string)?;
    let plaintext = serde_json::to_vec(document).map_err(error_string)?;
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&iv), plaintext.as_slice())
        .map_err(|_| "远端同步文件加密失败。".to_string())?;
    if encrypted.len() < 16 {
        return Err("远端同步文件加密失败。".to_string());
    }
    let (ciphertext, tag) = encrypted.split_at(encrypted.len() - 16);
    Ok(json!({
        "format": "shelldesk-sync-encrypted",
        "version": 1,
        "algorithm": "aes-256-gcm",
        "kdf": "pbkdf2-sha256",
        "iterations": 210000,
        "salt": base64::engine::general_purpose::STANDARD.encode(salt),
        "iv": base64::engine::general_purpose::STANDARD.encode(iv),
        "tag": base64::engine::general_purpose::STANDARD.encode(tag),
        "ciphertext": base64::engine::general_purpose::STANDARD.encode(ciphertext)
    }))
}

fn decrypt_remote_document(wrapper: &Value, passphrase: &str) -> Result<Value, String> {
    if wrapper.get("format").and_then(Value::as_str) != Some("shelldesk-sync-encrypted") {
        return Err("远端同步文件不是 ShellDesk 加密同步包。".to_string());
    }
    let decode = |key: &str| -> Result<Vec<u8>, String> {
        base64::engine::general_purpose::STANDARD
            .decode(wrapper.get(key).and_then(Value::as_str).unwrap_or(""))
            .map_err(|_| "同步密码不正确，或远端同步文件已损坏。".to_string())
    };
    let salt = decode("salt")?;
    let iv = decode("iv")?;
    let tag = decode("tag")?;
    let mut ciphertext = decode("ciphertext")?;
    let iterations = wrapper
        .get("iterations")
        .and_then(Value::as_u64)
        .unwrap_or(210_000)
        .clamp(100_000, 1_000_000) as u32;
    ciphertext.extend(tag);
    let key = derive_sync_key(passphrase, &salt, iterations);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(error_string)?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&iv), ciphertext.as_slice())
        .map_err(|_| "同步密码不正确，或远端同步文件已损坏。".to_string())?;
    serde_json::from_slice(&plaintext).map_err(|_| "远端同步文件内容无效。".to_string())
}

async fn read_remote_sync_document(config: &Value, secrets: &Value) -> Result<Value, String> {
    let remote_path = config
        .get("webdavRemotePath")
        .and_then(Value::as_str)
        .unwrap_or("/ShellDesk/shelldesk-sync.json");
    let response = webdav_request(config, secrets, "GET", remote_path, None, None, &[]).await?;
    if response.status().as_u16() == 404 {
        return Ok(json!({
            "document": create_empty_remote_document(),
            "etag": "",
            "exists": false
        }));
    }
    if !response.status().is_success() {
        return Err(webdav_response_error(response, "读取远端同步文件").await);
    }
    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = response.text().await.map_err(error_string)?;
    if text.is_empty() || text.len() > 25 * 1024 * 1024 {
        return Err("远端同步文件为空或超过大小限制。".to_string());
    }
    let raw: Value =
        serde_json::from_str(&text).map_err(|_| "远端同步文件内容无效。".to_string())?;
    let passphrase = secrets
        .get("syncPassphrase")
        .and_then(Value::as_str)
        .unwrap_or("");
    let decrypted = if raw.get("format").and_then(Value::as_str) == Some("shelldesk-sync-encrypted")
    {
        decrypt_remote_document(&raw, passphrase)?
    } else {
        raw
    };
    Ok(json!({
        "document": sanitize_remote_document(decrypted),
        "etag": etag,
        "exists": true
    }))
}

async fn write_remote_sync_document(
    config: &Value,
    secrets: &Value,
    document: &Value,
    etag: &str,
    exists: bool,
) -> Result<Value, String> {
    let passphrase = secrets
        .get("syncPassphrase")
        .and_then(Value::as_str)
        .unwrap_or("");
    let body_value = encrypt_remote_document(document, passphrase)?;
    let body = serde_json::to_string_pretty(&body_value).map_err(error_string)?;
    let remote_path = config
        .get("webdavRemotePath")
        .and_then(Value::as_str)
        .unwrap_or("/ShellDesk/shelldesk-sync.json");
    let response = webdav_request(
        config,
        secrets,
        "PUT",
        remote_path,
        Some(body),
        Some("application/json; charset=utf-8"),
        &webdav_write_precondition_headers(etag, exists),
    )
    .await?;
    if response.status().as_u16() == 412 {
        return Ok(json!({ "preconditionFailed": true, "etag": "" }));
    }
    if !matches!(response.status().as_u16(), 200 | 201 | 204) {
        return Err(webdav_response_error(response, "写入远端同步文件").await);
    }
    let next_etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    Ok(json!({ "preconditionFailed": false, "etag": next_etag }))
}

fn webdav_write_precondition_headers(etag: &str, exists: bool) -> Vec<(&'static str, String)> {
    if !etag.is_empty() {
        vec![("If-Match", etag.to_string())]
    } else if !exists {
        vec![("If-None-Match", "*".to_string())]
    } else {
        Vec::new()
    }
}

fn conflict_name(record: &Value) -> String {
    let payload = record.get("payload").unwrap_or(&Value::Null);
    match record.get("type").and_then(Value::as_str).unwrap_or("") {
        "host" => payload
            .get("name")
            .or_else(|| payload.get("address"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| record.get("id").and_then(Value::as_str).unwrap_or(""))
            .to_string(),
        "bookmark" => payload
            .pointer("/bookmark/title")
            .or_else(|| payload.pointer("/bookmark/url"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| record.get("id").and_then(Value::as_str).unwrap_or(""))
            .to_string(),
        "proxyProfile" => payload
            .get("name")
            .or_else(|| payload.pointer("/config/host"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| record.get("id").and_then(Value::as_str).unwrap_or(""))
            .to_string(),
        "knownHost" => format!(
            "{}:{}",
            payload
                .get("hostname")
                .and_then(Value::as_str)
                .unwrap_or(""),
            payload.get("port").and_then(Value::as_i64).unwrap_or(22)
        ),
        _ => "应用设置".to_string(),
    }
}

fn add_sync_conflict(conflicts: &mut Vec<Value>, record: &Value, reason: &str) {
    conflicts.push(json!({
        "type": record.get("type").and_then(Value::as_str).unwrap_or("host"),
        "id": record.get("id").and_then(Value::as_str).unwrap_or(""),
        "name": conflict_name(record),
        "reason": reason
    }));
}

fn tombstones_for_records(records: &Value, state: &Value, now_value: &str) -> Value {
    let mut tombstones = Map::new();
    if let Some(object) = records.as_object() {
        for (id, record) in object {
            let record_type = record.get("type").and_then(Value::as_str).unwrap_or("");
            if !synced_content_type(record_type) {
                continue;
            }
            tombstones.insert(
                id.clone(),
                json!({
                    "id": id,
                    "type": record_type,
                    "deletedAt": now_value,
                    "deviceId": state.get("deviceId").and_then(Value::as_str).unwrap_or("tauri-local"),
                    "hash": record.get("hash").and_then(Value::as_str).unwrap_or("")
                }),
            );
        }
    }
    Value::Object(tombstones)
}

fn merge_sync_documents(
    remote_document: &Value,
    local_records: &Value,
    local_tombstones: &Value,
    state: &Value,
    now_value: &str,
    conflict_resolution: &str,
) -> Value {
    let mut merged_records = remote_document
        .get("records")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut merged_tombstones = remote_document
        .get("tombstones")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let keep_local = conflict_resolution == "local";
    let keep_remote = conflict_resolution == "remote";
    let mut uploaded = 0;
    let mut downloaded = 0;
    let mut deleted = 0;
    let mut conflicts = Vec::new();

    if let Some(tombstones) = local_tombstones.as_object() {
        for (id, tombstone) in tombstones {
            let remote_record = merged_records.get(id).cloned();
            if remote_record.is_none()
                || remote_record
                    .as_ref()
                    .and_then(|record| record.get("hash"))
                    .and_then(Value::as_str)
                    == tombstone.get("hash").and_then(Value::as_str)
                || compare_time(
                    tombstone
                        .get("deletedAt")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    remote_record
                        .as_ref()
                        .and_then(|record| record.get("updatedAt"))
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                ) >= 0
            {
                merged_records.remove(id);
                merged_tombstones.insert(id.clone(), tombstone.clone());
                uploaded += 1;
                deleted += 1;
                continue;
            }
            if keep_local {
                if let Some(record) = remote_record.as_ref() {
                    add_sync_conflict(
                        &mut conflicts,
                        record,
                        "本机删除与远端修改冲突，已按选择保留本地：云端对应数据将删除。",
                    );
                }
                merged_records.remove(id);
                merged_tombstones.insert(id.clone(), tombstone.clone());
                uploaded += 1;
                deleted += 1;
            } else if let Some(record) = remote_record.as_ref() {
                add_sync_conflict(
                    &mut conflicts,
                    record,
                    if keep_remote {
                        "本机删除与远端修改冲突，已按选择保留云端：云端版本已恢复到本地。"
                    } else {
                        "本机删除与远端修改冲突，请选择保留本地或保留云端。"
                    },
                );
                if keep_remote {
                    downloaded += 1;
                }
            }
        }
    }

    if let Some(records) = local_records.as_object() {
        for (id, local_record) in records {
            if let Some(remote_tombstone) = merged_tombstones.get(id).cloned() {
                if compare_time(
                    remote_tombstone
                        .get("deletedAt")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    local_record
                        .get("updatedAt")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                ) >= 0
                {
                    let previous = state.pointer(&format!("/lastRecords/{}", escape_pointer(id)));
                    let local_changed = previous
                        .and_then(|value| value.get("hash"))
                        .and_then(Value::as_str)
                        .is_some_and(|hash| {
                            Some(hash) != local_record.get("hash").and_then(Value::as_str)
                        });
                    if !local_changed || keep_remote {
                        if keep_remote && local_changed {
                            add_sync_conflict(
                                &mut conflicts,
                                local_record,
                                "远端删除与本机修改冲突，已按选择保留云端：本机对应数据将删除。",
                            );
                        }
                        merged_records.remove(id);
                        downloaded += 1;
                        deleted += 1;
                        continue;
                    }
                    if keep_local {
                        add_sync_conflict(
                            &mut conflicts,
                            local_record,
                            "远端删除与本机修改冲突，已按选择保留本地：本机版本已覆盖到云端。",
                        );
                        merged_tombstones.remove(id);
                        merged_records.insert(id.clone(), local_record.clone());
                        uploaded += 1;
                        continue;
                    }
                    add_sync_conflict(
                        &mut conflicts,
                        local_record,
                        "远端删除与本机修改冲突，请选择保留本地或保留云端。",
                    );
                    continue;
                }
            }

            let remote_record = merged_records.get(id).cloned();
            if remote_record.is_none() {
                merged_records.insert(id.clone(), local_record.clone());
                uploaded += 1;
                continue;
            }
            let remote_record = remote_record.unwrap();
            if remote_record.get("hash").and_then(Value::as_str)
                == local_record.get("hash").and_then(Value::as_str)
            {
                if compare_time(
                    local_record
                        .get("updatedAt")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    remote_record
                        .get("updatedAt")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                ) > 0
                {
                    merged_records.insert(id.clone(), local_record.clone());
                }
                continue;
            }
            let previous = state.pointer(&format!("/lastRecords/{}", escape_pointer(id)));
            let local_changed = previous
                .and_then(|value| value.get("hash"))
                .and_then(Value::as_str)
                .is_some_and(|hash| Some(hash) != local_record.get("hash").and_then(Value::as_str));
            let remote_changed = previous
                .and_then(|value| value.get("hash"))
                .and_then(Value::as_str)
                .is_some_and(|hash| {
                    Some(hash) != remote_record.get("hash").and_then(Value::as_str)
                });
            if previous.is_some() && local_changed && remote_changed {
                let label = if local_record.get("type").and_then(Value::as_str) == Some("settings")
                {
                    "设置"
                } else {
                    "同一条数据"
                };
                if keep_remote {
                    add_sync_conflict(
                        &mut conflicts,
                        local_record,
                        &format!("{label}在本机和远端都被修改，已按选择保留云端版本。"),
                    );
                    downloaded += 1;
                } else if keep_local {
                    add_sync_conflict(
                        &mut conflicts,
                        local_record,
                        &format!("{label}在本机和远端都被修改，已按选择保留本地版本。"),
                    );
                    merged_records.insert(id.clone(), local_record.clone());
                    uploaded += 1;
                } else {
                    add_sync_conflict(
                        &mut conflicts,
                        local_record,
                        &format!("{label}在本机和远端都被修改，请选择保留本地或保留云端。"),
                    );
                }
                continue;
            }
            if compare_time(
                local_record
                    .get("updatedAt")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                remote_record
                    .get("updatedAt")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ) >= 0
            {
                merged_records.insert(id.clone(), local_record.clone());
                uploaded += 1;
            } else {
                downloaded += 1;
            }
        }
    }

    let mut devices = remote_document
        .get("devices")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let device_id = state
        .get("deviceId")
        .and_then(Value::as_str)
        .unwrap_or("tauri-local");
    devices.insert(
        device_id.to_string(),
        json!({
            "name": std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "ShellDesk".to_string()),
            "platform": node_platform(),
            "arch": std::env::consts::ARCH,
            "lastSeenAt": now_value
        }),
    );
    json!({
        "document": {
            "format": "shelldesk-sync-webdav",
            "version": 1,
            "updatedAt": now_value,
            "devices": Value::Object(devices),
            "records": Value::Object(merged_records),
            "tombstones": Value::Object(merged_tombstones)
        },
        "conflicts": conflicts,
        "uploaded": uploaded,
        "downloaded": downloaded,
        "deleted": deleted
    })
}

fn detect_suspicious_shrink(
    state: &Value,
    local_records: &Value,
    remote_document: &Value,
    merged_document: &Value,
) -> Option<Value> {
    let previous_counts = count_records_by_type(state.get("lastRecords").unwrap_or(&Value::Null));
    let local_counts = count_records_by_type(local_records);
    let remote_counts =
        count_records_by_type(remote_document.get("records").unwrap_or(&Value::Null));
    let merged_counts =
        count_records_by_type(merged_document.get("records").unwrap_or(&Value::Null));
    let previous = sum_counts(&previous_counts);
    let local = sum_counts(&local_counts);
    let remote = sum_counts(&remote_counts);
    let merged = sum_counts(&merged_counts);
    let baseline = previous.max(local).max(remote);
    let lost = baseline - merged;
    if baseline <= 0 || lost <= 0 {
        return None;
    }
    let suspicious = lost >= 10 || (lost >= 3 && merged <= baseline / 2);
    if !suspicious {
        return None;
    }
    let mut lost_by_type = Map::new();
    let mut keys = HashSet::new();
    for counts in [
        &previous_counts,
        &local_counts,
        &remote_counts,
        &merged_counts,
    ] {
        for key in counts.keys() {
            keys.insert(key.clone());
        }
    }
    for key in keys {
        let baseline_for_type = previous_counts
            .get(&key)
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(local_counts.get(&key).and_then(Value::as_i64).unwrap_or(0))
            .max(remote_counts.get(&key).and_then(Value::as_i64).unwrap_or(0));
        let lost_for_type =
            baseline_for_type - merged_counts.get(&key).and_then(Value::as_i64).unwrap_or(0);
        if lost_for_type > 0 {
            lost_by_type.insert(key, json!(lost_for_type));
        }
    }
    Some(json!({
        "baselineRecords": baseline,
        "mergedRecords": merged,
        "lostRecords": lost,
        "previousRecords": previous,
        "localRecords": local,
        "remoteRecords": remote,
        "lostByType": Value::Object(lost_by_type)
    }))
}

fn create_sync_state_from_document(
    document: &Value,
    device_id: &str,
    etag: &str,
    synced_at: &str,
) -> Value {
    let mut last_records = Map::new();
    if let Some(records) = document.get("records").and_then(Value::as_object) {
        for (id, record) in records {
            last_records.insert(
                id.clone(),
                json!({
                    "type": record.get("type").and_then(Value::as_str).unwrap_or("host"),
                    "hash": record.get("hash").and_then(Value::as_str).unwrap_or(""),
                    "updatedAt": record.get("updatedAt").and_then(Value::as_str).unwrap_or("")
                }),
            );
        }
    }
    let mut last_tombstones = Map::new();
    if let Some(tombstones) = document.get("tombstones").and_then(Value::as_object) {
        for (id, tombstone) in tombstones {
            last_tombstones.insert(id.clone(), tombstone.clone());
        }
    }
    json!({
        "deviceId": device_id,
        "lastRecords": Value::Object(last_records),
        "lastTombstones": Value::Object(last_tombstones),
        "lastSyncAt": synced_at,
        "lastRemoteEtag": etag
    })
}

fn records_array(document: &Value, record_type: &str) -> Vec<Value> {
    document
        .get("records")
        .and_then(Value::as_object)
        .map(|records| {
            records
                .values()
                .filter(|record| record.get("type").and_then(Value::as_str) == Some(record_type))
                .filter_map(|record| record.get("payload").cloned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn map_by_id(items: &[Value]) -> HashMap<String, Value> {
    items
        .iter()
        .filter_map(|item| Some((item.get("id")?.as_str()?.to_string(), item.clone())))
        .collect()
}

fn apply_sync_document_to_vault(state: &AppState, document: &Value) -> Result<Value, String> {
    let current = read_store(state)?;
    let current_hosts = current
        .get("hosts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let current_host_by_id = map_by_id(&current_hosts);
    let mut hosts = records_array(document, "host")
        .into_iter()
        .map(|mut host| {
            if let Some(current_host) = host
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| current_host_by_id.get(id))
            {
                if let Some(object) = host.as_object_mut() {
                    for key in ["password", "passphrase", "rootPassword"] {
                        object.insert(
                            key.to_string(),
                            current_host
                                .get(key)
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .into(),
                        );
                    }
                }
            }
            host
        })
        .collect::<Vec<_>>();
    hosts.sort_by_key(|item| {
        item.get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });

    let current_profiles = current
        .get("proxyProfiles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let current_profile_by_id = map_by_id(&current_profiles);
    let mut proxy_profiles = records_array(document, "proxyProfile")
        .into_iter()
        .map(|mut profile| {
            if let Some(current_profile) = profile
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| current_profile_by_id.get(id))
            {
                let password = current_profile
                    .pointer("/config/password")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(object) = profile.as_object_mut() {
                    let mut config = object
                        .get("config")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    config.insert("password".to_string(), json!(password));
                    object.insert("config".to_string(), Value::Object(config));
                }
            }
            profile
        })
        .collect::<Vec<_>>();
    proxy_profiles.sort_by_key(|item| {
        item.get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });

    let mut known_hosts = records_array(document, "knownHost");
    known_hosts.sort_by_key(|item| {
        format!(
            "{}:{}",
            item.get("hostname").and_then(Value::as_str).unwrap_or(""),
            item.get("port").and_then(Value::as_i64).unwrap_or(22)
        )
    });

    let mut settings = document
        .pointer("/records/settings:app/payload")
        .cloned()
        .unwrap_or_else(|| {
            current
                .get("settings")
                .cloned()
                .unwrap_or_else(default_settings)
        });
    if let Some(object) = settings.as_object_mut() {
        object.insert(
            "aiApiKey".to_string(),
            current
                .pointer("/settings/aiApiKey")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
        );
    }

    let mut bookmarks_by_scope: HashMap<String, Vec<Value>> = HashMap::new();
    for payload in records_array(document, "bookmark") {
        let scope = payload.get("scope").and_then(Value::as_str).unwrap_or("");
        if scope.is_empty() {
            continue;
        }
        if let Some(bookmark) = payload.get("bookmark") {
            bookmarks_by_scope
                .entry(scope.to_string())
                .or_default()
                .push(bookmark.clone());
        }
    }
    let mut browser_bookmarks = bookmarks_by_scope
        .into_iter()
        .map(|(scope, mut bookmarks)| {
            bookmarks.sort_by_key(|item| {
                item.get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            });
            json!({ "scope": scope, "bookmarks": bookmarks, "updatedAt": now() })
        })
        .collect::<Vec<_>>();
    browser_bookmarks.sort_by_key(|item| {
        item.get("scope")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });

    let mut next = current.clone();
    next["hosts"] = json!(hosts);
    next["settings"] = settings;
    next["proxyProfiles"] = json!(proxy_profiles);
    next["knownHosts"] = json!(known_hosts);
    next["browserBookmarks"] = json!(browser_bookmarks);
    write_store(state, &next)?;
    Ok(to_snapshot(state, next))
}

fn base_sync_result(
    store: &Value,
    local: &Value,
    remote_document: &Value,
    merged: &Value,
    ok: bool,
    snapshot_value: Value,
    message: &str,
) -> Value {
    let conflicts = merged
        .get("conflicts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let local_records = local.get("localRecords").unwrap_or(&Value::Null);
    let local_tombstones = local.get("localTombstones").unwrap_or(&Value::Null);
    let merged_document = merged.get("document").unwrap_or(remote_document);
    let uploaded = merged.get("uploaded").and_then(Value::as_i64).unwrap_or(0);
    let downloaded = merged
        .get("downloaded")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let deleted = merged.get("deleted").and_then(Value::as_i64).unwrap_or(0);
    json!({
        "ok": ok,
        "needsResolution": false,
        "needsEmptyVaultResolution": false,
        "needsShrinkConfirmation": false,
        "resolution": "",
        "emptyVaultResolution": "",
        "shrinkResolution": "",
        "syncedAt": if ok { now() } else { String::new() },
        "uploaded": uploaded,
        "downloaded": downloaded,
        "deleted": deleted,
        "conflictCount": conflicts.len(),
        "conflicts": conflicts,
        "conflictSummary": conflict_summary(&conflicts),
        "summary": sync_summary(local_records, local_tombstones, remote_document, merged_document, uploaded, downloaded, deleted, &conflicts),
        "emptyVaultSummary": null,
        "shrinkSummary": null,
        "snapshot": snapshot_value,
        "config": sync_public_config(store),
        "message": message
    })
}

fn pending_empty_vault_result(
    state: &AppState,
    store: &mut Value,
    local: &Value,
    remote_document: &Value,
) -> Result<Value, String> {
    let remote_count =
        count_content_records(remote_document.get("records").unwrap_or(&Value::Null));
    let message = format!(
        "本机 vault 为空，但云端有 {remote_count} 项数据。请选择恢复云端数据或保留本机空库。"
    );
    store["config"]["lastSyncStatus"] = json!("warning");
    store["config"]["lastSyncMessage"] = json!(message);
    store["config"]["lastConflictCount"] = json!(0);
    let saved = write_sync_store(state, store.clone())?;
    let mut result = base_sync_result(
        &saved,
        local,
        remote_document,
        &json!({ "document": remote_document, "conflicts": [], "uploaded": 0, "downloaded": 0, "deleted": 0 }),
        false,
        Value::Null,
        saved
            .pointer("/config/lastSyncMessage")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    result["needsEmptyVaultResolution"] = json!(true);
    result["emptyVaultSummary"] = json!({
        "localRecords": count_content_records(local.get("localRecords").unwrap_or(&Value::Null)),
        "remoteRecords": remote_count,
        "remoteRecordsByType": Value::Object(count_records_by_type(remote_document.get("records").unwrap_or(&Value::Null)))
    });
    Ok(result)
}

fn pending_conflict_result(
    state: &AppState,
    store: &mut Value,
    local: &Value,
    remote_document: &Value,
    merged: &Value,
) -> Result<Value, String> {
    let count = merged
        .get("conflicts")
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);
    let message = format!("发现 {count} 个同步冲突，请选择保留本地或保留云端。");
    store["config"]["lastSyncStatus"] = json!("warning");
    store["config"]["lastSyncMessage"] = json!(message);
    store["config"]["lastConflictCount"] = json!(count);
    let saved = write_sync_store(state, store.clone())?;
    let mut result = base_sync_result(
        &saved,
        local,
        remote_document,
        merged,
        false,
        Value::Null,
        saved
            .pointer("/config/lastSyncMessage")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    result["needsResolution"] = json!(true);
    Ok(result)
}

fn pending_shrink_result(
    state: &AppState,
    store: &mut Value,
    local: &Value,
    remote_document: &Value,
    merged: &Value,
    shrink: Option<Value>,
) -> Result<Value, String> {
    let shrink = shrink.unwrap_or(Value::Null);
    let message = format!(
        "同步结果会从 {} 项减少到 {} 项，已暂停以避免误删。",
        shrink
            .get("baselineRecords")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        shrink
            .get("mergedRecords")
            .and_then(Value::as_i64)
            .unwrap_or(0)
    );
    store["config"]["lastSyncStatus"] = json!("warning");
    store["config"]["lastSyncMessage"] = json!(message);
    store["config"]["lastConflictCount"] = json!(0);
    let saved = write_sync_store(state, store.clone())?;
    let mut result = base_sync_result(
        &saved,
        local,
        remote_document,
        merged,
        false,
        Value::Null,
        saved
            .pointer("/config/lastSyncMessage")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    result["needsShrinkConfirmation"] = json!(true);
    result["shrinkSummary"] = shrink;
    Ok(result)
}

fn sync_success_result(
    store: &Value,
    local: &Value,
    remote_document: &Value,
    merged: &Value,
    snapshot_value: Value,
    message: &str,
    conflict_resolution: &str,
    empty_vault_resolution: &str,
    shrink_resolution: &str,
) -> Value {
    let mut result = base_sync_result(
        store,
        local,
        remote_document,
        merged,
        true,
        snapshot_value,
        message,
    );
    result["resolution"] = json!(conflict_resolution);
    result["emptyVaultResolution"] = json!(empty_vault_resolution);
    result["shrinkResolution"] = json!(shrink_resolution);
    result
}

fn update_sync_status(state: &AppState, status: &str, message: &str) -> Result<(), String> {
    update_sync_status_with_time(state, status, message, "")
}

fn update_sync_status_with_time(
    state: &AppState,
    status: &str,
    message: &str,
    synced_at: &str,
) -> Result<(), String> {
    let mut store = read_sync_store(state)?;
    store["config"]["lastSyncStatus"] = json!(status);
    store["config"]["lastSyncMessage"] = json!(message);
    store["config"]["lastConflictCount"] = json!(0);
    if !synced_at.is_empty() {
        store["config"]["lastSyncAt"] = json!(synced_at);
    }
    let _ = write_sync_store(state, store)?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    fn electron_safe_storage_encrypt(plaintext: &str) -> String {
        let mut bytes = plaintext.as_bytes().to_vec();
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut output,
            )
        };
        assert_ne!(ok, 0);
        let encrypted = if output.pbData.is_null() || output.cbData == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() }
        };
        if !output.pbData.is_null() {
            unsafe {
                let _ = LocalFree(output.pbData as _);
            }
        }
        base64::engine::general_purpose::STANDARD.encode(encrypted)
    }

    #[test]
    fn decrypts_windows_dpapi_safe_storage_payload() {
        let plaintext = r#"{"webdavPassword":"webdav-secret","syncPassphrase":"sync-secret"}"#;
        let ciphertext = electron_safe_storage_encrypt(plaintext);
        let decrypted = decrypt_electron_safe_storage(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn normalizes_electron_protected_sync_store() {
        let ciphertext = electron_safe_storage_encrypt(
            r#"{"webdavPassword":"webdav-secret","syncPassphrase":"sync-secret"}"#,
        );
        let normalized = normalize_electron_protected_sync_store(json!({
            "format": "shelldesk-sync-settings",
            "version": 1,
            "protected": true,
            "ciphertext": ciphertext,
            "updatedAt": "2026-01-01T00:00:00.000Z",
            "config": {
                "enabled": true,
                "webdavUrl": "https://dav.example.com/root",
                "webdavUsername": "alice",
                "webdavRemotePath": "/ShellDesk/sync.json",
                "intervalMinutes": 20
            },
            "state": {
                "deviceId": "device-1",
                "lastRecords": {},
                "lastTombstones": {}
            }
        }))
        .unwrap();

        assert_eq!(
            normalized
                .pointer("/secrets/webdavPassword")
                .and_then(Value::as_str),
            Some("webdav-secret")
        );
        assert_eq!(
            normalized
                .pointer("/secrets/syncPassphrase")
                .and_then(Value::as_str),
            Some("sync-secret")
        );
        assert_eq!(
            normalized
                .pointer("/config/webdavUsername")
                .and_then(Value::as_str),
            Some("alice")
        );
        assert_eq!(
            normalized
                .pointer("/state/deviceId")
                .and_then(Value::as_str),
            Some("device-1")
        );
    }

    #[test]
    fn webdav_write_precondition_headers_match_legacy_etag_rules() {
        assert_eq!(
            webdav_write_precondition_headers("\"abc\"", true),
            vec![("If-Match", "\"abc\"".to_string())]
        );
        assert_eq!(
            webdav_write_precondition_headers("", false),
            vec![("If-None-Match", "*".to_string())]
        );
        assert!(webdav_write_precondition_headers("", true).is_empty());
    }

    #[test]
    fn sync_footprint_tracks_record_and_tombstone_hashes_only() {
        let first = create_sync_footprint(
            &json!({
                "host:1": {
                    "id": "host:1",
                    "type": "host",
                    "hash": "hash-a",
                    "payload": { "name": "alpha" }
                }
            }),
            &json!({
                "bookmark:1": {
                    "id": "bookmark:1",
                    "type": "bookmark",
                    "hash": "hash-b",
                    "deletedAt": "2026-06-18T00:00:00.000Z"
                }
            }),
        );
        let second = create_sync_footprint(
            &json!({
                "host:1": {
                    "id": "host:1",
                    "type": "host",
                    "hash": "hash-a",
                    "payload": { "name": "renamed but same hash" }
                }
            }),
            &json!({
                "bookmark:1": {
                    "id": "bookmark:1",
                    "type": "bookmark",
                    "hash": "hash-b",
                    "deletedAt": "2026-06-19T00:00:00.000Z"
                }
            }),
        );
        let changed = create_sync_footprint(
            &json!({
                "host:1": {
                    "id": "host:1",
                    "type": "host",
                    "hash": "hash-c",
                    "payload": { "name": "alpha" }
                }
            }),
            &json!({
                "bookmark:1": {
                    "id": "bookmark:1",
                    "type": "bookmark",
                    "hash": "hash-b"
                }
            }),
        );

        assert_eq!(first, second);
        assert_ne!(first, changed);
    }
}

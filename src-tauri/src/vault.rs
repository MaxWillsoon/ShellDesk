use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, env, fs, process::Stdio};
use tauri::Emitter;
use tokio::process::Command;

use crate::{error_string, now, prevent_tokio_process_window, random_id, vault_storage, AppState};

const MAX_PRIVATE_KEY_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PUBLIC_KEY_BYTES: u64 = 128 * 1024;

fn default_store(state: &AppState) -> Value {
    json!({
        "hosts": [],
        "sshKeys": [],
        "proxyProfiles": [],
        "knownHosts": [],
        "settings": default_settings(),
        "browserBookmarks": [],
        "remoteConnectionProfiles": {},
        "preferences": {},
        "storage": storage_info(state)
    })
}

pub(crate) fn default_settings() -> Value {
    let language = default_language();
    json!({
        "language": language,
        "interfaceFont": "Microsoft YaHei UI",
        "theme": "dark",
        "accentColor": "#0f6bff",
        "defaultHostView": "grid",
        "minimizeToTrayOnClose": true,
        "autoUpdateEnabled": true,
        "desktopWallpaperMode": "preset",
        "desktopWallpaperPresetId": "default",
        "desktopWallpaperDataUrl": "",
        "desktopWallpaperName": "",
        "remoteDesktopLayout": {
            "appCatalogVersion": 9,
            "sortMode": "custom",
            "items": [
                { "id": "app:files", "type": "app", "appKey": "files" },
                { "id": "app:terminal", "type": "app", "appKey": "terminal" },
                { "id": "app:browser", "type": "app", "appKey": "browser" },
                { "id": "app:settings", "type": "app", "appKey": "settings" }
            ]
        },
        "rememberPasswords": true,
        "rememberKeyPassphrases": true,
        "aiProvider": "openai",
        "aiProviderName": "OpenAI",
        "aiApiFormat": "openai",
        "aiApiBaseUrl": "https://api.openai.com/v1",
        "aiApiKey": "",
        "aiModel": "",
        "terminalFontSize": 13,
        "terminalFontFamily": "Cascadia Mono",
        "terminalFontWeight": 400,
        "terminalFontWeightBold": 700,
        "terminalLigatures": true,
        "terminalFontLigatures": true,
        "terminalLineHeight": 1.2,
        "terminalTheme": "shelldesk-dark",
        "terminalCursorBlink": true,
        "terminalCursorStyle": "block",
        "terminalCursorInactiveStyle": "outline",
        "terminalScrollback": 10000,
        "terminalScrollSensitivity": 1,
        "terminalFastScrollSensitivity": 5,
        "terminalScrollOnUserInput": true,
        "terminalScrollOnEraseInDisplay": true,
        "terminalCopyOnSelect": true,
        "terminalRightClickPaste": true,
        "terminalAltClickMovesCursor": true,
        "terminalBracketedPasteMode": true,
        "terminalMinimumContrastRatio": 1,
        "terminalScreenReaderMode": false,
        "terminalSnippets": default_terminal_snippets(language)
    })
}

fn default_terminal_snippets(language: &str) -> Value {
    let is_chinese = language == "zh-CN";
    let group = if is_chinese {
        "常用巡检"
    } else {
        "Common Checks"
    };
    let snippets = if is_chinese {
        vec![
            ("system-overview", "系统概览", "uname -a && uptime"),
            ("disk-usage", "磁盘占用", "df -h"),
            ("memory-usage", "内存占用", "free -h"),
            (
                "listening-ports",
                "监听端口",
                "ss -tulpen || netstat -tulpen",
            ),
            ("recent-logins", "最近登录", "last -a | head -20"),
        ]
    } else {
        vec![
            ("system-overview", "System overview", "uname -a && uptime"),
            ("disk-usage", "Disk usage", "df -h"),
            ("memory-usage", "Memory usage", "free -h"),
            (
                "listening-ports",
                "Listening ports",
                "ss -tulpen || netstat -tulpen",
            ),
            ("recent-logins", "Recent logins", "last -a | head -20"),
        ]
    };
    Value::Array(
        snippets
            .into_iter()
            .map(|(id, label, command)| {
                json!({
                    "id": format!("builtin:{id}"),
                    "label": label,
                    "command": command,
                    "group": group,
                    "shortcut": "",
                    "createdAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                })
            })
            .collect(),
    )
}

fn default_language() -> &'static str {
    detect_system_language().unwrap_or("en-US")
}

fn detect_system_language() -> Option<&'static str> {
    for key in [
        "SHELLDESK_LANGUAGE",
        "LC_ALL",
        "LC_MESSAGES",
        "LANG",
        "LANGUAGE",
    ] {
        if let Ok(value) = env::var(key) {
            if let Some(language) = language_from_locale(&value) {
                return Some(language);
            }
        }
    }
    platform_system_language()
}

fn language_from_locale(locale: &str) -> Option<&'static str> {
    let normalized = locale.trim().replace('_', "-").to_ascii_lowercase();
    if normalized.is_empty() || normalized == "c" || normalized == "posix" {
        return None;
    }
    let primary = normalized
        .split(['-', '.', '@', ':'])
        .next()
        .unwrap_or("")
        .trim();
    if primary == "zh" {
        Some("zh-CN")
    } else {
        Some("en-US")
    }
}

#[cfg(windows)]
fn platform_system_language() -> Option<&'static str> {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    let language_id = unsafe { GetUserDefaultUILanguage() };
    let primary_language = language_id & 0x03ff;
    if primary_language == 0x04 {
        Some("zh-CN")
    } else if language_id != 0 {
        Some("en-US")
    } else {
        None
    }
}

#[cfg(not(windows))]
fn platform_system_language() -> Option<&'static str> {
    None
}

fn storage_info(state: &AppState) -> Value {
    vault_storage::storage_info(state)
}

pub(crate) fn read_store(state: &AppState) -> Result<Value, String> {
    let defaults = default_store(state);
    let (mut store, should_rewrite) = vault_storage::read_store(state, defaults.clone())?;
    merge_defaults(&mut store, defaults);
    store["storage"] = storage_info(state);
    if should_rewrite {
        write_store(state, &store)?;
    }
    Ok(store)
}

pub(crate) fn write_store(state: &AppState, store: &Value) -> Result<(), String> {
    vault_storage::write_store(state, store)
}

pub(crate) fn snapshot(state: &AppState) -> Result<Value, String> {
    let store = read_store(state)?;
    Ok(to_snapshot(state, store))
}

pub(crate) fn public_snapshot(state: &AppState) -> Result<Value, String> {
    let store = read_store(state)?;
    Ok(to_public_snapshot(state, store))
}

pub(crate) fn to_snapshot(state: &AppState, store: Value) -> Value {
    json!({
        "hosts": store.get("hosts").cloned().unwrap_or_else(|| json!([])),
        "sshKeys": public_ssh_keys(store.get("sshKeys")),
        "proxyProfiles": store.get("proxyProfiles").cloned().unwrap_or_else(|| json!([])),
        "knownHosts": store.get("knownHosts").cloned().unwrap_or_else(|| json!([])),
        "settings": store.get("settings").cloned().unwrap_or_else(default_settings),
        "browserBookmarks": store.get("browserBookmarks").cloned().unwrap_or_else(|| json!([])),
        "storage": storage_info(state)
    })
}

fn to_public_snapshot(state: &AppState, store: Value) -> Value {
    json!({
        "hosts": public_hosts(store.get("hosts")),
        "sshKeys": public_ssh_keys_without_secrets(store.get("sshKeys")),
        "proxyProfiles": public_proxy_profiles(store.get("proxyProfiles")),
        "knownHosts": store.get("knownHosts").cloned().unwrap_or_else(|| json!([])),
        "settings": public_settings(store.get("settings").cloned().unwrap_or_else(default_settings)),
        "browserBookmarks": store.get("browserBookmarks").cloned().unwrap_or_else(|| json!([])),
        "storage": storage_info(state)
    })
}

fn public_hosts(hosts: Option<&Value>) -> Value {
    let items = hosts.and_then(Value::as_array).cloned().unwrap_or_default();
    Value::Array(
        items
            .into_iter()
            .map(|mut item| {
                if let Some(object) = item.as_object_mut() {
                    object.insert("password".to_string(), json!(""));
                    object.insert("passphrase".to_string(), json!(""));
                    object.insert("rootPassword".to_string(), json!(""));
                }
                item
            })
            .collect(),
    )
}

fn public_ssh_keys(keys: Option<&Value>) -> Value {
    let items = keys.and_then(Value::as_array).cloned().unwrap_or_default();
    Value::Array(
        items
            .into_iter()
            .map(|mut item| {
                if let Some(object) = item.as_object_mut() {
                    object.remove("privateKey");
                }
                item
            })
            .collect(),
    )
}

fn public_ssh_keys_without_secrets(keys: Option<&Value>) -> Value {
    let items = keys.and_then(Value::as_array).cloned().unwrap_or_default();
    Value::Array(
        items
            .into_iter()
            .map(|mut item| {
                if let Some(object) = item.as_object_mut() {
                    object.remove("privateKey");
                    object.insert("passphrase".to_string(), json!(""));
                }
                item
            })
            .collect(),
    )
}

fn public_proxy_profiles(profiles: Option<&Value>) -> Value {
    let items = profiles
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Value::Array(
        items
            .into_iter()
            .map(|mut item| {
                if let Some(config) = item
                    .as_object_mut()
                    .and_then(|object| object.get_mut("config"))
                    .and_then(Value::as_object_mut)
                {
                    config.insert("password".to_string(), json!(""));
                }
                item
            })
            .collect(),
    )
}

fn public_settings(mut settings: Value) -> Value {
    if let Some(object) = settings.as_object_mut() {
        object.insert("aiApiKey".to_string(), json!(""));
    }
    settings
}

fn renderer_key_record(mut key: Value) -> Value {
    if let Some(object) = key.as_object_mut() {
        object.remove("privateKey");
    }
    key
}

pub(crate) fn merge_private_key_fields(
    existing: Option<&Value>,
    incoming: &Value,
) -> Result<Value, String> {
    let existing_keys = existing
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let private_by_id = existing_keys
        .iter()
        .filter_map(|item| {
            Some((
                item.get("id")?.as_str()?.to_string(),
                item.get("privateKey")?.as_str()?.to_string(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let incoming_keys = incoming.as_array().cloned().unwrap_or_default();
    let mut merged = Vec::with_capacity(incoming_keys.len());
    for mut item in incoming_keys {
        if item
            .get("privateKey")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("");
            if let Some(private_key) = private_by_id.get(id) {
                item["privateKey"] = json!(private_key);
            } else {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(id);
                return Err(format!("密钥「{}」缺少私钥内容，无法保存。", name));
            }
        }
        merged.push(item);
    }
    Ok(Value::Array(merged))
}

pub(crate) fn upsert_vault_collections(
    store: &mut Value,
    raw_payload: Value,
) -> Result<(), String> {
    let Some(payload) = raw_payload.as_object() else {
        return Err("本地数据无效。".to_string());
    };

    let Some(store_object) = store.as_object_mut() else {
        return Err("本地数据无效。".to_string());
    };

    if let Some(value) = payload.get("hosts").filter(|value| value.is_array()) {
        store_object.insert("hosts".to_string(), normalize_hosts(value)?);
    }

    if let Some(value) = payload
        .get("proxyProfiles")
        .filter(|value| value.is_array())
    {
        store_object.insert(
            "proxyProfiles".to_string(),
            normalize_proxy_profiles(value)?,
        );
    }

    if let Some(value) = payload.get("knownHosts").filter(|value| value.is_array()) {
        store_object.insert("knownHosts".to_string(), normalize_known_hosts(value)?);
    }

    if let Some(value) = payload.get("settings") {
        store_object.insert("settings".to_string(), normalize_app_settings(value)?);
    }

    if let Some(value) = payload.get("sshKeys").filter(|value| value.is_array()) {
        let merged = normalize_ssh_keys_for_store(store_object.get("sshKeys"), value)?;
        store_object.insert("sshKeys".to_string(), merged);
    }

    Ok(())
}

#[path = "vault/normalize.rs"]
mod normalize;

pub(crate) use normalize::{
    normalize_app_settings, normalize_hosts, normalize_known_hosts, normalize_proxy_profiles,
    normalize_ssh_keys_for_import, normalize_ssh_keys_for_store,
};
fn merge_defaults(target: &mut Value, defaults: Value) {
    let Some(target_object) = target.as_object_mut() else {
        *target = defaults;
        return;
    };
    if let Some(default_object) = defaults.as_object() {
        for (key, value) in default_object {
            match target_object.get_mut(key) {
                Some(existing) if existing.is_object() && value.is_object() => {
                    merge_defaults(existing, value.clone());
                }
                Some(_) => {}
                None => {
                    target_object.insert(key.clone(), value.clone());
                }
            }
        }
    }
}

pub(crate) fn get_bookmarks(store: &Value, raw_scope: &str) -> Result<Value, String> {
    let scope = read_bookmark_scope(raw_scope)?;
    Ok(store
        .get("browserBookmarks")
        .and_then(Value::as_array)
        .and_then(|collections| {
            collections.iter().find(|collection| {
                collection
                    .get("scope")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == scope)
            })
        })
        .and_then(|collection| collection.get("bookmarks"))
        .cloned()
        .unwrap_or_else(|| json!([])))
}

pub(crate) fn save_bookmarks_to_store(
    store: &mut Value,
    raw_scope: &str,
    raw_bookmarks: Value,
) -> Result<Value, String> {
    let scope = read_bookmark_scope(raw_scope)?;
    let bookmarks = read_browser_bookmarks(&raw_bookmarks)?;
    let Some(collections) = store
        .as_object_mut()
        .and_then(|object| object.get_mut("browserBookmarks"))
        .and_then(Value::as_array_mut)
    else {
        return Err("书签分组无效。".to_string());
    };

    let updated_at = now();
    let mut next_collections = Vec::new();
    if !bookmarks.as_array().is_some_and(Vec::is_empty) {
        next_collections.push(json!({
            "scope": scope,
            "bookmarks": bookmarks.clone(),
            "updatedAt": updated_at
        }));
    }
    next_collections.extend(
        collections
            .iter()
            .filter(|collection| {
                !collection
                    .get("scope")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == scope)
            })
            .cloned(),
    );
    *collections = next_collections;
    Ok(bookmarks)
}

fn read_bookmark_scope(value: &str) -> Result<String, String> {
    read_bounded_string(value, "书签范围", 255, true, true, true)
}

fn read_browser_bookmarks(value: &Value) -> Result<Value, String> {
    let Some(bookmarks) = value.as_array() else {
        return Ok(json!([]));
    };
    bookmarks
        .iter()
        .map(read_browser_bookmark)
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn read_browser_bookmark(value: &Value) -> Result<Value, String> {
    let Some(bookmark) = value.as_object() else {
        return Err("浏览器书签无效。".to_string());
    };
    Ok(json!({
        "id": read_bounded_string_value(bookmark.get("id"), "书签 ID", 128, true, true, true)?,
        "title": read_bounded_string_value(bookmark.get("title"), "书签名称", 200, true, true, true)?,
        "url": read_bounded_string_value(bookmark.get("url"), "书签地址", 4096, true, true, true)?,
        "createdAt": read_bounded_string_value(bookmark.get("createdAt"), "书签创建时间", 64, true, true, true)?,
        "updatedAt": read_bounded_string_value(bookmark.get("updatedAt"), "书签更新时间", 64, true, true, true)?
    }))
}

fn read_bounded_string_value(
    value: Option<&Value>,
    label: &str,
    max_length: usize,
    required: bool,
    trim: bool,
    reject_line_breaks: bool,
) -> Result<String, String> {
    let Some(Value::String(value)) = value else {
        return Err(format!("{label}无效。"));
    };
    read_bounded_string(value, label, max_length, required, trim, reject_line_breaks)
}

fn read_bounded_string(
    value: &str,
    label: &str,
    max_length: usize,
    required: bool,
    trim: bool,
    reject_line_breaks: bool,
) -> Result<String, String> {
    let next_value = if trim {
        value.trim().to_string()
    } else {
        value.to_string()
    };
    if required && next_value.is_empty() {
        return Err(format!("请输入{}。", label));
    }
    if next_value.chars().count() > max_length
        || next_value.contains('\0')
        || (reject_line_breaks && next_value.contains(['\r', '\n']))
    {
        return Err(format!("{}无效。", label));
    }
    Ok(next_value)
}

pub(crate) fn get_preference(store: &Value, raw_key: &str) -> Result<Value, String> {
    let key = read_preference_key(raw_key)?;
    Ok(store
        .get("preferences")
        .and_then(|preferences| preferences.get(&key))
        .cloned()
        .unwrap_or(Value::Null))
}

pub(crate) fn set_preference_to_store(
    store: &mut Value,
    raw_key: &str,
    raw_value: Value,
) -> Result<Value, String> {
    let key = read_preference_key(raw_key)?;
    let value = read_preference_value(raw_value)?;
    let Some(store_object) = store.as_object_mut() else {
        return Err("本地数据无效。".to_string());
    };
    if !store_object
        .get("preferences")
        .is_some_and(Value::is_object)
    {
        store_object.insert("preferences".to_string(), json!({}));
    }
    let preferences = store_object
        .get_mut("preferences")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "本地数据无效。".to_string())?;
    if value.is_null() {
        preferences.remove(&key);
    } else {
        preferences.insert(key.clone(), value.clone());
    }
    Ok(preferences.get(&key).cloned().unwrap_or(Value::Null))
}

fn read_preference_key(value: &str) -> Result<String, String> {
    let key = read_bounded_string(value, "偏好设置键", 255, true, true, true)?;
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '.' | '_' | '%' | '-'))
    {
        return Err("偏好设置键无效。".to_string());
    }
    Ok(key)
}

fn read_preference_value(value: Value) -> Result<Value, String> {
    let serialized = serde_json::to_vec(&value).map_err(error_string)?;
    if serialized.len() > 64 * 1024 {
        return Err("偏好设置内容无效或超过大小限制。".to_string());
    }
    serde_json::from_slice(&serialized).map_err(|_| "偏好设置内容无效或超过大小限制。".to_string())
}

const REMOTE_DESKTOP_APP_KEYS: &[&str] = &[
    "files",
    "terminal",
    "notepad",
    "browser",
    "vnc",
    "log-viewer",
    "monitor",
    "mysql",
    "clickhouse",
    "redis",
    "service-manager",
    "container-manager",
    "port-manager",
    "firewall-manager",
    "iptables-manager",
    "network-diagnostics",
    "disk-analyzer",
    "disk-manager",
    "package-manager",
    "git-manager",
    "cert-manager",
    "nginx-manager",
    "caddy-manager",
    "apache-manager",
    "scheduled-tasks",
    "postgres",
    "mongo",
    "search-cluster",
    "message-queue",
    "s3-browser",
    "security-audit",
    "login-sessions",
    "api-debugger",
    "procmanager",
    "settings",
    "sqlite",
];

pub(crate) fn get_remote_connection_profile(
    store: &Value,
    raw_host_id: &str,
    raw_app_key: &str,
) -> Result<Value, String> {
    let host_id = read_remote_connection_profile_host_id(raw_host_id)?;
    let app_key = read_remote_connection_profile_app_key(raw_app_key)?;
    Ok(store
        .get("remoteConnectionProfiles")
        .and_then(|profiles| profiles.get(&host_id))
        .and_then(|profiles| profiles.get(&app_key))
        .cloned()
        .unwrap_or(Value::Null))
}

pub(crate) fn save_remote_connection_profile_to_store(
    store: &mut Value,
    raw_host_id: &str,
    raw_app_key: &str,
    raw_values: Value,
) -> Result<Value, String> {
    let host_id = read_remote_connection_profile_host_id(raw_host_id)?;
    let app_key = read_remote_connection_profile_app_key(raw_app_key)?;
    let values = read_remote_connection_profile_values(raw_values)?;
    let Some(store_object) = store.as_object_mut() else {
        return Err("本地数据无效。".to_string());
    };
    if !store_object
        .get("remoteConnectionProfiles")
        .is_some_and(Value::is_object)
    {
        store_object.insert("remoteConnectionProfiles".to_string(), json!({}));
    }
    let profiles = store_object
        .get_mut("remoteConnectionProfiles")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "本地数据无效。".to_string())?;
    let host_profiles = profiles
        .entry(host_id)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "本地数据无效。".to_string())?;
    host_profiles.insert(app_key, values.clone());
    Ok(values)
}

fn read_remote_connection_profile_host_id(value: &str) -> Result<String, String> {
    read_bounded_string(value, "远程组件主机 ID", 512, true, true, true)
}

fn read_remote_connection_profile_app_key(value: &str) -> Result<String, String> {
    let app_key = read_bounded_string(value, "远程组件标识", 80, true, true, true)?;
    if !REMOTE_DESKTOP_APP_KEYS.contains(&app_key.as_str()) {
        return Err("远程组件标识无效。".to_string());
    }
    Ok(app_key)
}

fn read_remote_connection_profile_values(raw_values: Value) -> Result<Value, String> {
    let Some(values) = raw_values.as_object() else {
        return Ok(json!({}));
    };
    let serialized = serde_json::to_vec(&raw_values).map_err(error_string)?;
    if serialized.len() > 64 * 1024 {
        return Err("远程组件连接配置超过大小限制。".to_string());
    }
    let mut output = serde_json::Map::new();
    for (raw_key, raw_value) in values.iter().take(80) {
        let key = read_bounded_string(raw_key, "远程组件配置键", 80, true, true, true)?;
        if !key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-'))
        {
            continue;
        }
        output.insert(
            key.clone(),
            read_remote_connection_profile_value(raw_value, &format!("远程组件配置 {key}"))?,
        );
    }
    Ok(Value::Object(output))
}

fn read_remote_connection_profile_value(value: &Value, label: &str) -> Result<Value, String> {
    match value {
        Value::String(value) => Ok(json!(read_bounded_string(
            value, label, 8192, false, false, false
        )?)),
        Value::Bool(_) => Ok(value.clone()),
        Value::Number(number) if number.as_f64().is_some_and(f64::is_finite) => Ok(value.clone()),
        _ => Ok(json!("")),
    }
}

pub(crate) async fn import_key_pair(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Imported key")
        .to_string();
    let public_key_path = payload
        .get("publicKeyPath")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut public_key = if public_key_path.is_empty() {
        String::new()
    } else {
        read_local_text_file(&public_key_path, "SSH 公钥", MAX_PUBLIC_KEY_BYTES)?
    };
    let private_key_path = payload
        .get("privateKeyPath")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let private_key = read_local_text_file(&private_key_path, "SSH 私钥", MAX_PRIVATE_KEY_BYTES)?;
    let passphrase = payload
        .get("passphrase")
        .and_then(Value::as_str)
        .unwrap_or("");
    if public_key.trim().is_empty() {
        public_key = derive_public_key_from_private_key(&private_key_path, passphrase)
            .await
            .unwrap_or_default();
    }
    let fingerprint = public_key_fingerprint(&public_key).unwrap_or_default();
    let algorithm = public_key_algorithm(&public_key)
        .unwrap_or("unknown")
        .to_string();
    let key = json!({
        "id": random_id("key"),
        "name": name,
        "source": "imported",
        "algorithm": algorithm,
        "fingerprint": fingerprint,
        "publicKey": public_key.trim(),
        "privateKey": private_key,
        "passphrase": passphrase,
        "createdAt": now(),
        "updatedAt": now()
    });
    let mut store = read_store(state)?;
    ensure_unique_ssh_key(store.get("sshKeys"), &key)?;
    if let Some(keys) = store.get_mut("sshKeys").and_then(Value::as_array_mut) {
        keys.push(key.clone());
    }
    write_store(state, &store)?;
    let _ = window.emit("vault:changed", json!({ "kind": "vault" }));
    Ok(json!({ "snapshot": to_snapshot(state, store), "key": renderer_key_record(key) }))
}

pub(crate) async fn generate_key_pair(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("RSA key")
        .to_string();
    let modulus_length = payload
        .get("modulusLength")
        .and_then(Value::as_u64)
        .filter(|value| matches!(*value, 2048 | 3072 | 4096))
        .unwrap_or(4096)
        .to_string();
    let passphrase = payload
        .get("passphrase")
        .and_then(Value::as_str)
        .unwrap_or("");
    let key_id = random_id("key");
    let key_dir = state.data_dir.join("generated-keys");
    fs::create_dir_all(&key_dir).map_err(error_string)?;
    let private_path = key_dir.join(&key_id);
    let public_path = key_dir.join(format!("{key_id}.pub"));
    if private_path.exists() {
        let _ = fs::remove_file(&private_path);
    }
    if public_path.exists() {
        let _ = fs::remove_file(&public_path);
    }
    let mut command = Command::new("ssh-keygen");
    prevent_tokio_process_window(&mut command);
    let output = command
        .args([
            "-t",
            "rsa",
            "-b",
            &modulus_length,
            "-N",
            passphrase,
            "-C",
            &name,
            "-f",
            &private_path.to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(error_string)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let private_key = fs::read_to_string(&private_path).map_err(error_string)?;
    let public_key = fs::read_to_string(&public_path).unwrap_or_default();
    let _ = fs::remove_file(&private_path);
    let _ = fs::remove_file(&public_path);
    let fingerprint = public_key_fingerprint(&public_key).unwrap_or_default();
    let algorithm = public_key_algorithm(&public_key)
        .unwrap_or("RSA")
        .to_string();
    let key = json!({
        "id": key_id,
        "name": name,
        "source": "generated",
        "algorithm": algorithm,
        "fingerprint": fingerprint,
        "publicKey": public_key.trim(),
        "privateKey": private_key,
        "passphrase": passphrase,
        "createdAt": now(),
        "updatedAt": now()
    });
    let mut store = read_store(state)?;
    if let Some(keys) = store.get_mut("sshKeys").and_then(Value::as_array_mut) {
        keys.push(key.clone());
    }
    write_store(state, &store)?;
    let _ = window.emit("vault:changed", json!({ "kind": "vault" }));
    Ok(json!({ "snapshot": to_snapshot(state, store), "key": renderer_key_record(key) }))
}

fn read_local_text_file(path: &str, label: &str, max_bytes: u64) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return Err(format!("{label}路径无效。"));
    }
    let metadata = fs::metadata(trimmed).map_err(|_| format!("{label}不存在。"))?;
    if !metadata.is_file() {
        return Err(format!("{label}不存在。"));
    }
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(format!("{label}为空或超过大小限制。"));
    }
    fs::read_to_string(trimmed).map_err(error_string)
}

async fn derive_public_key_from_private_key(
    private_key_path: &str,
    passphrase: &str,
) -> Result<String, String> {
    let mut command = Command::new("ssh-keygen");
    prevent_tokio_process_window(&mut command);
    let output = command
        .args(["-y", "-f", private_key_path, "-P", passphrase])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(error_string)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn public_key_algorithm(public_key: &str) -> Option<&str> {
    public_key.split_whitespace().next().filter(|value| {
        value.starts_with("ssh-") || value.starts_with("ecdsa-") || value.starts_with("sk-")
    })
}

fn public_key_fingerprint(public_key: &str) -> Option<String> {
    let mut parts = public_key.split_whitespace();
    let algorithm = parts.next()?;
    let encoded_key = parts.next()?;
    if !(algorithm.starts_with("ssh-")
        || algorithm.starts_with("ecdsa-")
        || algorithm.starts_with("sk-"))
    {
        return None;
    }
    let key_bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded_key)
        .ok()?;
    let digest = Sha256::digest(key_bytes);
    let fingerprint = base64::engine::general_purpose::STANDARD
        .encode(digest)
        .trim_end_matches('=')
        .to_string();
    Some(format!("SHA256:{fingerprint}"))
}

fn ensure_unique_ssh_key(existing: Option<&Value>, next_key: &Value) -> Result<(), String> {
    let next_private_key = next_key
        .get("privateKey")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let next_fingerprint = next_key
        .get("fingerprint")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    for key in existing.and_then(Value::as_array).into_iter().flatten() {
        if !next_private_key.is_empty()
            && key
                .get("privateKey")
                .and_then(Value::as_str)
                .is_some_and(|value| value.trim() == next_private_key)
        {
            return Err("这个 SSH 私钥已经在密钥库中。".to_string());
        }
        if !next_fingerprint.is_empty()
            && key
                .get("fingerprint")
                .and_then(Value::as_str)
                .is_some_and(|value| value.trim() == next_fingerprint)
        {
            return Err("这个 SSH 私钥已经在密钥库中。".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSA_PUBLIC_KEY: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC7";

    #[test]
    fn public_key_fingerprint_matches_openssh_sha256_format() {
        assert_eq!(
            public_key_fingerprint(RSA_PUBLIC_KEY),
            Some("SHA256:HPlRPaJS3AalL0f2B3TkvOVkd9tmwMs8k9hR+TLJWRQ".to_string())
        );
        assert_eq!(public_key_algorithm(RSA_PUBLIC_KEY), Some("ssh-rsa"));
    }

    #[test]
    fn renderer_key_record_removes_private_key() {
        let key = renderer_key_record(json!({
            "id": "key-1",
            "name": "prod",
            "privateKey": "private",
            "publicKey": RSA_PUBLIC_KEY
        }));
        assert!(key.get("privateKey").is_none());
        assert_eq!(key["publicKey"], RSA_PUBLIC_KEY);
    }

    #[test]
    fn public_snapshot_helpers_remove_secrets() {
        let hosts = public_hosts(Some(&json!([{
            "id": "host-1",
            "password": "secret",
            "passphrase": "phrase",
            "rootPassword": "root"
        }])));
        assert_eq!(hosts[0]["password"], "");
        assert_eq!(hosts[0]["passphrase"], "");
        assert_eq!(hosts[0]["rootPassword"], "");

        let keys = public_ssh_keys_without_secrets(Some(&json!([{
            "id": "key-1",
            "privateKey": "private",
            "passphrase": "phrase",
            "publicKey": RSA_PUBLIC_KEY
        }])));
        assert!(keys[0].get("privateKey").is_none());
        assert_eq!(keys[0]["passphrase"], "");
        assert_eq!(keys[0]["publicKey"], RSA_PUBLIC_KEY);

        let profiles = public_proxy_profiles(Some(&json!([{
            "id": "proxy-1",
            "config": { "type": "http", "password": "secret" }
        }])));
        assert_eq!(profiles[0]["config"]["password"], "");

        let settings = public_settings(json!({ "aiApiKey": "sk-test", "language": "zh-CN" }));
        assert_eq!(settings["aiApiKey"], "");
        assert_eq!(settings["language"], "zh-CN");
    }

    #[test]
    fn default_settings_preserve_legacy_host_view() {
        let settings = default_settings();
        assert_eq!(settings["defaultHostView"], "grid");
        assert!(settings["autoUpdateEnabled"].as_bool().unwrap_or(false));
        assert_eq!(settings["terminalSnippets"].as_array().unwrap().len(), 5);
        assert_eq!(
            settings["terminalSnippets"][0]["id"],
            "builtin:system-overview"
        );
    }

    #[test]
    fn normalize_app_settings_matches_legacy_ranges_and_fallbacks() {
        let settings = normalize_app_settings(&json!({
            "language": "fr-FR",
            "interfaceFont": "  Cascadia   Code  ",
            "theme": "blue",
            "accentColor": "#ABCDEF",
            "defaultHostView": "table",
            "minimizeToTrayOnClose": "yes",
            "autoUpdateEnabled": false,
            "desktopWallpaperMode": "custom",
            "desktopWallpaperPresetId": "missing",
            "desktopWallpaperDataUrl": "",
            "remoteDesktopLayout": {
                "appCatalogVersion": 9,
                "sortMode": "bad",
                "items": [
                    { "type": "app", "appKey": "terminal" },
                    { "type": "app", "appKey": "terminal" },
                    { "type": "app", "appKey": "bad-app" },
                    { "type": "folder", "id": "", "name": "", "appKeys": ["files", "bad-app", "terminal"] }
                ]
            },
            "rememberPasswords": false,
            "aiProvider": "anthropic",
            "aiProviderName": "",
            "terminalFontSize": 99,
            "terminalFontFamily": "  JetBrains   Mono  ",
            "terminalFontWeight": 200,
            "terminalFontWeightBold": 900,
            "terminalLineHeight": 2,
            "terminalTheme": "unknown",
            "terminalCursorStyle": "block",
            "terminalCursorInactiveStyle": "bad",
            "terminalScrollback": 1,
            "terminalScrollSensitivity": 9,
            "terminalFastScrollSensitivity": 1,
            "terminalMinimumContrastRatio": 9,
            "terminalSnippets": [
                { "id": "same", "label": "", "command": "ignored" },
                { "id": "same", "label": "Deploy", "command": "echo ok\n", "shortcut": "Ctrl+ Shift + D" },
                { "id": "same", "label": "Logs", "command": "tail -f app.log" }
            ]
        }))
        .unwrap();

        assert!(matches!(
            settings["language"].as_str(),
            Some("zh-CN" | "en-US")
        ));
        assert_eq!(settings["interfaceFont"], "Cascadia Code");
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["accentColor"], "#abcdef");
        assert_eq!(settings["defaultHostView"], "grid");
        assert_eq!(settings["minimizeToTrayOnClose"], true);
        assert_eq!(settings["autoUpdateEnabled"], false);
        assert_eq!(settings["desktopWallpaperMode"], "preset");
        assert_eq!(settings["desktopWallpaperPresetId"], "default");
        assert_eq!(settings["remoteDesktopLayout"]["sortMode"], "custom");
        assert_eq!(
            settings["remoteDesktopLayout"]["items"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            settings["remoteDesktopLayout"]["items"][0]["appKey"],
            "terminal"
        );
        assert_eq!(
            settings["remoteDesktopLayout"]["items"][1]["appKeys"],
            json!(["files"])
        );
        assert_eq!(settings["aiProvider"], "anthropic");
        assert_eq!(settings["aiApiFormat"], "anthropic");
        assert_eq!(settings["aiApiBaseUrl"], "https://api.anthropic.com");
        assert_eq!(settings["aiProviderName"], "Claude / Anthropic");
        assert_eq!(settings["terminalFontSize"], 13);
        assert_eq!(settings["terminalFontFamily"], "JetBrains Mono");
        assert_eq!(settings["terminalFontWeight"], 400);
        assert_eq!(settings["terminalFontWeightBold"], 700);
        assert_eq!(settings["terminalLineHeight"], 1.2);
        assert_eq!(settings["terminalTheme"], "shelldesk-dark");
        assert_eq!(settings["terminalCursorStyle"], "block");
        assert_eq!(settings["terminalCursorInactiveStyle"], "outline");
        assert_eq!(settings["terminalScrollback"], 10000);
        assert_eq!(settings["terminalScrollSensitivity"], 1.0);
        assert_eq!(settings["terminalFastScrollSensitivity"], 5);
        assert_eq!(settings["terminalMinimumContrastRatio"], 1.0);
        let snippets = settings["terminalSnippets"].as_array().unwrap();
        assert_eq!(snippets.len(), 2);
        assert_eq!(snippets[0]["id"], "same");
        assert_ne!(snippets[1]["id"], "same");
        assert_eq!(snippets[0]["command"], "echo ok");
        assert_eq!(snippets[0]["shortcut"], "Ctrl + Shift + D");
    }

    #[test]
    fn normalize_app_settings_rejects_invalid_urls_and_wallpapers() {
        assert_eq!(
            normalize_app_settings(&json!({ "aiApiBaseUrl": "ftp://example.com" })).unwrap_err(),
            "AI API 地址只支持 http 或 https。"
        );
        assert_eq!(
            normalize_app_settings(&json!({
                "desktopWallpaperMode": "custom",
                "desktopWallpaperDataUrl": "data:text/plain;base64,SGVsbG8="
            }))
            .unwrap_err(),
            "桌面壁纸无效。"
        );
    }

    #[test]
    fn normalize_proxy_profiles_matches_legacy_shapes() {
        let profiles = normalize_proxy_profiles(&json!([
            {
                "id": " proxy-1 ",
                "label": " Corp HTTP ",
                "config": {
                    "type": "http",
                    "host": " proxy.example.com ",
                    "port": "8080",
                    "username": " user ",
                    "password": " secret\nvalue ",
                    "command": "ignored"
                },
                "createdAt": "2026-01-01T00:00:00.000Z"
            },
            {
                "id": "proxy-command",
                "label": "Command",
                "config": {
                    "type": "command",
                    "command": "  nc -x proxy:1080 %h %p\n",
                    "host": "ignored",
                    "port": 9999,
                    "username": "ignored",
                    "password": "ignored"
                },
                "createdAt": "2026-01-02T00:00:00.000Z",
                "updatedAt": "2026-01-03T00:00:00.000Z"
            }
        ]))
        .unwrap();

        assert_eq!(profiles[0]["id"], "proxy-1");
        assert_eq!(profiles[0]["label"], "Corp HTTP");
        assert_eq!(profiles[0]["config"]["type"], "http");
        assert_eq!(profiles[0]["config"]["host"], "proxy.example.com");
        assert_eq!(profiles[0]["config"]["port"], 8080);
        assert_eq!(profiles[0]["config"]["username"], "user");
        assert_eq!(profiles[0]["config"]["password"], " secret\nvalue ");
        assert_eq!(profiles[0]["config"]["command"], "");
        assert_eq!(profiles[0]["updatedAt"], "2026-01-01T00:00:00.000Z");
        assert_eq!(profiles[1]["config"]["host"], "");
        assert_eq!(profiles[1]["config"]["port"], 0);
        assert_eq!(profiles[1]["config"]["command"], "nc -x proxy:1080 %h %p");
        assert_eq!(profiles[1]["config"]["username"], "");
        assert_eq!(profiles[1]["config"]["password"], "");
    }

    #[test]
    fn normalize_proxy_profiles_rejects_invalid_records() {
        assert_eq!(
            normalize_proxy_profiles(&json!([{ "id": "proxy-1" }])).unwrap_err(),
            "代理名称无效。"
        );
        assert_eq!(
            normalize_proxy_profiles(&json!([{
                "id": "proxy-1",
                "label": "Proxy",
                "config": { "type": "http", "host": "proxy.example.com", "port": 0 },
                "createdAt": "2026-01-01T00:00:00.000Z"
            }]))
            .unwrap_err(),
            "代理端口无效。"
        );
        assert_eq!(
            normalize_proxy_profiles(&json!([{
                "id": "proxy-1",
                "label": "Proxy",
                "config": { "type": "ftp" },
                "createdAt": "2026-01-01T00:00:00.000Z"
            }]))
            .unwrap_err(),
            "代理类型无效。"
        );
    }

    #[test]
    fn normalize_known_hosts_matches_legacy_shapes() {
        let known_hosts = normalize_known_hosts(&json!([
            {
                "id": " known-1 ",
                "hostname": " example.com ",
                "port": "2222",
                "keyType": " ssh-ed25519 ",
                "publicKey": " ssh-ed25519 AAAA\ncomment ",
                "fingerprint": " SHA256:abc ",
                "discoveredAt": "2026-01-01T00:00:00.000Z",
                "lastSeen": "2026-01-02T00:00:00.000Z",
                "convertedToHostId": " host-1 "
            }
        ]))
        .unwrap();

        assert_eq!(known_hosts[0]["id"], "known-1");
        assert_eq!(known_hosts[0]["hostname"], "example.com");
        assert_eq!(known_hosts[0]["port"], 2222);
        assert_eq!(known_hosts[0]["keyType"], "ssh-ed25519");
        assert_eq!(known_hosts[0]["publicKey"], "ssh-ed25519 AAAA\ncomment");
        assert_eq!(known_hosts[0]["fingerprint"], "SHA256:abc");
        assert_eq!(known_hosts[0]["lastSeen"], "2026-01-02T00:00:00.000Z");
        assert_eq!(known_hosts[0]["convertedToHostId"], "host-1");
    }

    #[test]
    fn normalize_known_hosts_rejects_invalid_records() {
        assert_eq!(
            normalize_known_hosts(&json!([null])).unwrap_err(),
            "已知主机数据无效。"
        );
        assert_eq!(
            normalize_known_hosts(&json!([{
                "id": "known-1",
                "hostname": "example.com",
                "port": 70000,
                "discoveredAt": "2026-01-01T00:00:00.000Z"
            }]))
            .unwrap_err(),
            "已知主机端口无效。"
        );
    }

    fn host_record(id: &str, name: &str, created_at: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "address": format!("{id}.example.com"),
            "port": 22,
            "username": "root",
            "authMethod": "password",
            "password": "secret",
            "privilegeMode": "sudo",
            "createdAt": created_at,
            "updatedAt": created_at
        })
    }

    fn find_record<'a>(items: &'a Value, id: &str) -> &'a Value {
        items
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
            .unwrap()
    }

    #[test]
    fn normalize_hosts_scrubs_auth_fields_and_sorts_like_legacy_list() {
        let mut password_host = host_record("host-password", "Password", "2026-01-02T00:00:00Z");
        password_host["keyId"] = json!("stale-key");
        password_host["keyPath"] = json!("C:/Users/test/.ssh/id_rsa");
        password_host["passphrase"] = json!("stale-passphrase");
        password_host["rootPassword"] = json!("stale-root");
        password_host["hostInfo"] = json!({
            "address": "other.example.com",
            "collectedAt": "2026-01-02T00:00:00Z",
            "systemType": "ubuntu",
            "items": [{ "key": "kernel", "label": "Kernel", "value": "6.8" }]
        });

        let mut key_host = host_record("host-key", "Key", "2026-01-01T00:00:00Z");
        key_host["authMethod"] = json!("key");
        key_host["password"] = json!("stale-password");
        key_host["keyId"] = json!("key-1");
        key_host["passphrase"] = json!("key-passphrase");
        key_host["privilegeMode"] = json!("su-root");
        key_host["rootPassword"] = json!("root-secret");
        key_host["systemType"] = json!("UBUNTU");
        key_host["hostInfo"] = json!({
            "address": "host-key.example.com",
            "collectedAt": "2026-01-01T00:00:00Z",
            "systemType": "debian",
            "systemName": "Debian",
            "items": [{ "key": "os", "label": "OS", "value": "Debian 12", "icon": "pc" }]
        });

        let hosts = normalize_hosts(&json!([key_host, password_host])).unwrap();

        assert_eq!(hosts[0]["id"], "host-password");
        assert_eq!(hosts[1]["id"], "host-key");
        assert_eq!(hosts[0]["keyId"], "");
        assert_eq!(hosts[0]["keyPath"], "");
        assert_eq!(hosts[0]["passphrase"], "");
        assert_eq!(hosts[0]["rootPassword"], "");
        assert!(hosts[0]["hostInfo"].is_null());
        assert_eq!(hosts[1]["password"], "");
        assert_eq!(hosts[1]["keyId"], "key-1");
        assert_eq!(hosts[1]["passphrase"], "key-passphrase");
        assert_eq!(hosts[1]["rootPassword"], "root-secret");
        assert_eq!(hosts[1]["systemType"], "ubuntu");
        assert_eq!(hosts[1]["hostInfo"]["items"][0]["value"], "Debian 12");
    }

    #[test]
    fn normalize_hosts_cleans_invalid_jump_host_references() {
        let jump = host_record("jump", "Jump", "2026-01-04T00:00:00Z");
        let mut via_jump = host_record("via-jump", "Via Jump", "2026-01-03T00:00:00Z");
        via_jump["jumpHostId"] = json!("jump");
        let mut nested = host_record("nested", "Nested", "2026-01-02T00:00:00Z");
        nested["jumpHostId"] = json!("via-jump");
        let mut self_jump = host_record("self", "Self", "2026-01-01T00:00:00Z");
        self_jump["jumpHostId"] = json!("self");
        let mut missing = host_record("missing", "Missing", "2026-01-01T00:00:01Z");
        missing["jumpHostId"] = json!("no-such-host");

        let hosts = normalize_hosts(&json!([nested, via_jump, jump, self_jump, missing])).unwrap();

        assert_eq!(find_record(&hosts, "jump")["canBeJumpHost"], true);
        assert_eq!(find_record(&hosts, "via-jump")["canBeJumpHost"], true);
        assert_eq!(find_record(&hosts, "via-jump")["jumpHostId"], "jump");
        assert_eq!(find_record(&hosts, "nested")["jumpHostId"], "");
        assert_eq!(find_record(&hosts, "self")["jumpHostId"], "");
        assert_eq!(find_record(&hosts, "missing")["jumpHostId"], "");
    }

    #[test]
    fn normalize_hosts_rejects_invalid_auth_records() {
        let mut missing_auth = host_record("host-1", "Host 1", "2026-01-01T00:00:00Z");
        missing_auth.as_object_mut().unwrap().remove("authMethod");
        assert_eq!(
            normalize_hosts(&json!([missing_auth])).unwrap_err(),
            "主机登录方式无效。"
        );

        let mut key_without_secret = host_record("host-2", "Key Host", "2026-01-01T00:00:00Z");
        key_without_secret["authMethod"] = json!("key");
        key_without_secret["password"] = json!("");
        assert_eq!(
            normalize_hosts(&json!([key_without_secret])).unwrap_err(),
            "主机「Key Host」缺少私钥信息。"
        );
    }

    #[test]
    fn normalize_ssh_keys_merges_private_key_and_validates_content() {
        let existing = json!([{
            "id": "key-1",
            "name": "Prod",
            "source": "generated",
            "algorithm": "",
            "fingerprint": "",
            "publicKey": RSA_PUBLIC_KEY,
            "passphrase": "old-passphrase",
            "privateKey": "-----BEGIN PRIVATE KEY-----\nprivate\n-----END PRIVATE KEY-----",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z"
        }]);
        let incoming = json!([{
            "id": "key-1",
            "name": "Prod",
            "source": "generated",
            "algorithm": "",
            "fingerprint": "",
            "publicKey": RSA_PUBLIC_KEY,
            "passphrase": "new-passphrase",
            "privateKey": "",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z"
        }]);

        let keys = normalize_ssh_keys_for_store(Some(&existing), &incoming).unwrap();
        assert_eq!(keys[0]["algorithm"], "RSA");
        assert_eq!(keys[0]["passphrase"], "new-passphrase");
        assert_eq!(
            keys[0]["privateKey"],
            "-----BEGIN PRIVATE KEY-----\nprivate\n-----END PRIVATE KEY-----"
        );

        let invalid_key = json!([{
            "id": "key-2",
            "name": "Invalid",
            "privateKey": "not a private key",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z"
        }]);
        assert_eq!(
            normalize_ssh_keys_for_import(&invalid_key).unwrap_err(),
            "SSH 私钥内容无效。"
        );
    }

    #[test]
    fn language_from_locale_matches_legacy_language_choices() {
        assert_eq!(language_from_locale("zh-CN"), Some("zh-CN"));
        assert_eq!(language_from_locale("zh_Hans_CN.UTF-8"), Some("zh-CN"));
        assert_eq!(language_from_locale("en-US"), Some("en-US"));
        assert_eq!(language_from_locale("fr-FR"), Some("en-US"));
        assert_eq!(language_from_locale("C"), None);
        assert_eq!(language_from_locale(""), None);
    }

    #[test]
    fn bookmarks_save_matches_legacy_normalization_and_ordering() {
        let mut store = json!({
            "browserBookmarks": [
                {
                    "scope": "old-scope",
                    "bookmarks": [{
                        "id": "old",
                        "title": "Old",
                        "url": "https://old.example",
                        "createdAt": "2026-01-01T00:00:00.000Z",
                        "updatedAt": "2026-01-01T00:00:00.000Z"
                    }],
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                }
            ]
        });

        let saved = save_bookmarks_to_store(
            &mut store,
            " browser:host-1 ",
            json!([{
                "id": " bookmark-1 ",
                "title": " ShellDesk ",
                "url": " https://example.com ",
                "createdAt": "2026-01-01T00:00:00.000Z",
                "updatedAt": "2026-01-02T00:00:00.000Z"
            }]),
        )
        .unwrap();

        assert_eq!(saved[0]["id"], "bookmark-1");
        assert_eq!(saved[0]["title"], "ShellDesk");
        assert_eq!(saved[0]["url"], "https://example.com");
        assert_eq!(store["browserBookmarks"][0]["scope"], "browser:host-1");
        assert_eq!(store["browserBookmarks"][0]["bookmarks"], saved);
        assert_eq!(store["browserBookmarks"][1]["scope"], "old-scope");
        assert_eq!(
            get_bookmarks(&store, "browser:host-1").unwrap()[0]["id"],
            "bookmark-1"
        );
    }

    #[test]
    fn bookmarks_save_removes_empty_collection() {
        let mut store = json!({
            "browserBookmarks": [
                {
                    "scope": "browser:host-1",
                    "bookmarks": [{
                        "id": "bookmark-1",
                        "title": "ShellDesk",
                        "url": "https://example.com",
                        "createdAt": "2026-01-01T00:00:00.000Z",
                        "updatedAt": "2026-01-01T00:00:00.000Z"
                    }],
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                },
                {
                    "scope": "browser:host-2",
                    "bookmarks": [],
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                }
            ]
        });

        let saved = save_bookmarks_to_store(&mut store, "browser:host-1", json!([])).unwrap();

        assert!(saved.as_array().unwrap().is_empty());
        assert_eq!(store["browserBookmarks"].as_array().unwrap().len(), 1);
        assert_eq!(store["browserBookmarks"][0]["scope"], "browser:host-2");
    }

    #[test]
    fn bookmarks_reject_invalid_records_and_scopes() {
        let mut store = json!({ "browserBookmarks": [] });
        assert_eq!(
            save_bookmarks_to_store(
                &mut store,
                "browser:host-1",
                json!([{ "id": "missing-fields" }])
            )
            .unwrap_err(),
            "书签名称无效。"
        );
        assert_eq!(
            get_bookmarks(&store, "bad\nscope").unwrap_err(),
            "书签范围无效。"
        );
    }

    #[test]
    fn preferences_set_get_and_delete_match_legacy_semantics() {
        let mut store = json!({ "preferences": {} });

        let saved = set_preference_to_store(
            &mut store,
            " terminal.font-size ",
            json!({ "value": 14, "enabled": true }),
        )
        .unwrap();

        assert_eq!(saved["value"], 14);
        assert_eq!(
            get_preference(&store, "terminal.font-size").unwrap()["enabled"],
            true
        );

        let deleted =
            set_preference_to_store(&mut store, "terminal.font-size", Value::Null).unwrap();

        assert!(deleted.is_null());
        assert!(get_preference(&store, "terminal.font-size")
            .unwrap()
            .is_null());
    }

    #[test]
    fn preferences_reject_invalid_keys_and_oversized_values() {
        let mut store = json!({ "preferences": {} });

        assert_eq!(
            get_preference(&store, "bad key").unwrap_err(),
            "偏好设置键无效。"
        );
        assert_eq!(
            set_preference_to_store(&mut store, "", json!(true)).unwrap_err(),
            "请输入偏好设置键。"
        );

        let oversized = "x".repeat(64 * 1024 + 1);
        assert_eq!(
            set_preference_to_store(&mut store, "terminal.theme", json!(oversized)).unwrap_err(),
            "偏好设置内容无效或超过大小限制。"
        );
    }

    #[test]
    fn preferences_create_missing_store_object() {
        let mut store = json!({});

        let saved = set_preference_to_store(&mut store, "sidebar.width", json!(320)).unwrap();

        assert_eq!(saved, json!(320));
        assert_eq!(store["preferences"]["sidebar.width"], 320);
    }

    #[test]
    fn remote_connection_profiles_normalize_values_and_round_trip() {
        let mut store = json!({ "remoteConnectionProfiles": {} });

        let saved = save_remote_connection_profile_to_store(
            &mut store,
            " host-1 ",
            "mysql",
            json!({
                "host": " 127.0.0.1 ",
                "password": " secret\nvalue ",
                "ssl": true,
                "port": 3306,
                "nested": { "ignored": true },
                "bad key": "skipped"
            }),
        )
        .unwrap();

        assert_eq!(saved["host"], " 127.0.0.1 ");
        assert_eq!(saved["password"], " secret\nvalue ");
        assert_eq!(saved["ssl"], true);
        assert_eq!(saved["port"], 3306);
        assert_eq!(saved["nested"], "");
        assert!(saved.get("bad key").is_none());
        assert_eq!(
            get_remote_connection_profile(&store, "host-1", "mysql").unwrap(),
            saved
        );
        assert!(get_remote_connection_profile(&store, "host-1", "redis")
            .unwrap()
            .is_null());
    }

    #[test]
    fn remote_connection_profiles_reject_invalid_app_key_and_large_values() {
        let mut store = json!({ "remoteConnectionProfiles": {} });

        assert_eq!(
            save_remote_connection_profile_to_store(&mut store, "host-1", "unknown-app", json!({}))
                .unwrap_err(),
            "远程组件标识无效。"
        );

        let oversized = "x".repeat(64 * 1024 + 1);
        assert_eq!(
            save_remote_connection_profile_to_store(
                &mut store,
                "host-1",
                "mysql",
                json!({ "payload": oversized })
            )
            .unwrap_err(),
            "远程组件连接配置超过大小限制。"
        );
    }

    #[test]
    fn remote_connection_profiles_limit_items_and_validate_key_length() {
        let mut many_values = serde_json::Map::new();
        for index in 0..90 {
            many_values.insert(format!("key-{index}"), json!(index));
        }
        let values = read_remote_connection_profile_values(Value::Object(many_values)).unwrap();
        assert_eq!(values.as_object().unwrap().len(), 80);

        let long_key = "x".repeat(81);
        assert_eq!(
            read_remote_connection_profile_values(json!({ long_key: true })).unwrap_err(),
            "远程组件配置键无效。"
        );
    }

    #[test]
    fn merge_private_key_fields_restores_existing_private_key() {
        let existing = json!([
            { "id": "key-1", "name": "prod", "privateKey": "private" }
        ]);
        let incoming = json!([
            { "id": "key-1", "name": "prod", "privateKey": "", "publicKey": RSA_PUBLIC_KEY }
        ]);

        let merged = merge_private_key_fields(Some(&existing), &incoming).unwrap();
        assert_eq!(merged[0]["privateKey"], "private");
        assert_eq!(merged[0]["publicKey"], RSA_PUBLIC_KEY);
    }

    #[test]
    fn merge_private_key_fields_rejects_new_key_without_private_key() {
        let incoming = json!([
            { "id": "key-new", "name": "new key", "privateKey": "" }
        ]);

        assert_eq!(
            merge_private_key_fields(Some(&json!([])), &incoming).unwrap_err(),
            "密钥「new key」缺少私钥内容，无法保存。"
        );
    }

    #[test]
    fn upsert_vault_collections_preserves_missing_collections_and_normalizes_settings() {
        let mut store = json!({
            "hosts": [{ "id": "host-existing" }],
            "sshKeys": [{ "id": "key-1", "name": "prod", "privateKey": "private" }],
            "proxyProfiles": [{ "id": "proxy-existing" }],
            "knownHosts": [{ "host": "old.example.com" }],
            "settings": { "language": "zh-CN", "theme": "dark" }
        });

        upsert_vault_collections(
            &mut store,
            json!({
                "hosts": [{
                    "id": "host-next",
                    "name": "Next",
                    "address": "example.com",
                    "port": "22",
                    "username": "root",
                    "authMethod": "password",
                    "password": "secret",
                    "keyId": "stale-key",
                    "passphrase": "stale-passphrase",
                    "privilegeMode": "sudo",
                    "rootPassword": "should-clear",
                    "group": "",
                    "tags": [],
                    "note": "",
                    "createdAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-02T00:00:00.000Z"
                }],
                "proxyProfiles": null,
                "knownHosts": "invalid",
                "settings": null
            }),
        )
        .unwrap();

        assert_eq!(store["hosts"][0]["id"], "host-next");
        assert_eq!(store["hosts"][0]["port"], 22);
        assert_eq!(store["hosts"][0]["keyId"], "");
        assert_eq!(store["hosts"][0]["passphrase"], "");
        assert_eq!(store["hosts"][0]["rootPassword"], "");
        assert_eq!(store["proxyProfiles"][0]["id"], "proxy-existing");
        assert_eq!(store["knownHosts"][0]["host"], "old.example.com");
        assert_eq!(store["settings"]["defaultHostView"], "grid");
        assert_eq!(store["settings"]["terminalFontSize"], 13);
        assert_eq!(store["sshKeys"][0]["privateKey"], "private");
    }

    #[test]
    fn upsert_vault_collections_merges_settings_defaults_and_key_secrets() {
        let mut store = json!({
            "hosts": [],
            "sshKeys": [{
                "id": "key-1",
                "name": "prod",
                "privateKey": "-----BEGIN PRIVATE KEY-----\nprivate\n-----END PRIVATE KEY-----"
            }],
            "proxyProfiles": [],
            "knownHosts": [],
            "settings": { "language": "en-US", "theme": "dark" }
        });

        upsert_vault_collections(
            &mut store,
            json!({
                "settings": { "language": "zh-CN" },
                "proxyProfiles": [{
                    "id": " proxy-1 ",
                    "label": " Proxy ",
                    "config": { "type": "socks5", "host": " 127.0.0.1 ", "port": "1080" },
                    "createdAt": "2026-01-01T00:00:00.000Z"
                }],
                "knownHosts": [{
                    "id": " known-1 ",
                    "hostname": " example.com ",
                    "port": "22",
                    "discoveredAt": "2026-01-01T00:00:00.000Z"
                }],
                "sshKeys": [{
                    "id": "key-1",
                    "name": "prod",
                    "source": "imported",
                    "algorithm": "",
                    "fingerprint": "",
                    "privateKey": "",
                    "publicKey": RSA_PUBLIC_KEY,
                    "passphrase": "phrase",
                    "createdAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-02T00:00:00.000Z"
                }]
            }),
        )
        .unwrap();

        assert_eq!(store["settings"]["language"], "zh-CN");
        assert_eq!(store["settings"]["defaultHostView"], "grid");
        assert_eq!(store["proxyProfiles"][0]["id"], "proxy-1");
        assert_eq!(store["proxyProfiles"][0]["config"]["host"], "127.0.0.1");
        assert_eq!(store["proxyProfiles"][0]["config"]["port"], 1080);
        assert_eq!(store["knownHosts"][0]["id"], "known-1");
        assert_eq!(store["knownHosts"][0]["hostname"], "example.com");
        assert_eq!(store["knownHosts"][0]["port"], 22);
        assert_eq!(
            store["sshKeys"][0]["privateKey"],
            "-----BEGIN PRIVATE KEY-----\nprivate\n-----END PRIVATE KEY-----"
        );
        assert_eq!(store["sshKeys"][0]["algorithm"], "SSH");
        assert_eq!(store["sshKeys"][0]["publicKey"], RSA_PUBLIC_KEY);
    }

    #[test]
    fn upsert_vault_collections_rejects_invalid_payload_and_missing_key_secret() {
        let mut store = json!({
            "sshKeys": []
        });

        assert_eq!(
            upsert_vault_collections(&mut store, json!(null)).unwrap_err(),
            "本地数据无效。"
        );
        assert_eq!(
            upsert_vault_collections(
                &mut store,
                json!({ "sshKeys": [{ "id": "key-new", "name": "new key", "privateKey": "" }] })
            )
            .unwrap_err(),
            "密钥「new key」缺少私钥内容，无法保存。"
        );
    }

    #[test]
    fn ensure_unique_ssh_key_rejects_duplicate_private_key() {
        let existing = json!([
            { "id": "key-1", "privateKey": " private\n", "fingerprint": "" }
        ]);
        let next = json!({ "privateKey": "private", "fingerprint": "" });
        assert_eq!(
            ensure_unique_ssh_key(Some(&existing), &next).unwrap_err(),
            "这个 SSH 私钥已经在密钥库中。"
        );
    }

    #[test]
    fn ensure_unique_ssh_key_rejects_duplicate_fingerprint() {
        let existing = json!([
            { "id": "key-1", "privateKey": "", "fingerprint": "SHA256:abc" }
        ]);
        let next = json!({ "privateKey": "other", "fingerprint": "SHA256:abc" });
        assert_eq!(
            ensure_unique_ssh_key(Some(&existing), &next).unwrap_err(),
            "这个 SSH 私钥已经在密钥库中。"
        );
    }
}

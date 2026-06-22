use crate::vault::{read_store, write_store};
use crate::{
    command_exists, error_string, now, prevent_tokio_process_window, random_id,
    run_ssh_command_for_profile_with_window, shell_quote, AppState, SshProfile,
};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, future::Future, pin::Pin, process::Stdio, time::Duration};
use tauri::Emitter;
use tokio::{process::Command, sync::oneshot, time};

pub(crate) fn respond_host_key_verification(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let request_id = payload
        .get("requestId")
        .and_then(Value::as_str)
        .ok_or_else(|| "主机密钥确认请求无效。".to_string())?
        .to_string();
    let sender = state
        .host_key_responses
        .lock()
        .map_err(error_string)?
        .remove(&request_id)
        .ok_or_else(|| "主机密钥确认请求已过期。".to_string())?;
    sender
        .send(payload)
        .map_err(|_| "主机密钥确认请求已关闭。".to_string())?;
    Ok(json!(true))
}

pub(super) fn ensure_ssh_host_key_trusted<'a>(
    state: &'a AppState,
    window: &'a tauri::Window,
    profile: &'a mut SshProfile,
) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        if let Some(jump) = profile.jump.as_deref_mut() {
            ensure_ssh_host_key_trusted(state, window, jump).await?;
        }
        ensure_direct_ssh_host_key_trusted(state, window, profile).await
    })
}

pub(crate) async fn ensure_ssh_profile_host_key_trusted(
    state: &AppState,
    window: &tauri::Window,
    profile: &mut SshProfile,
) -> Result<(), String> {
    ensure_ssh_host_key_trusted(state, window, profile).await
}

pub(crate) async fn confirm_ssh_host_public_key_trusted(
    state: &AppState,
    window: &tauri::Window,
    hostname: &str,
    port: u16,
    username: &str,
    public_key: &str,
) -> Result<bool, String> {
    let store = read_store(state)?;
    let known_hosts = store
        .get("knownHosts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let scanned = scanned_host_key_from_public_key(public_key)?;
    let decision = classify_scanned_host_key(&known_hosts, hostname, port, &scanned);
    match decision
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
    {
        "trusted" => Ok(true),
        "unknown" | "changed" => {
            let profile = SshProfile {
                address: hostname.to_string(),
                port,
                username: username.to_string(),
                auth_method: String::new(),
                password: String::new(),
                key_path: String::new(),
                known_hosts_path: String::new(),
                proxy_helper_exe: String::new(),
                proxy: None,
                jump: None,
            };
            let response =
                request_host_key_decision(state, window, &profile, &scanned, &decision).await?;
            let accept = response
                .get("accept")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if accept
                && response
                    .get("addToKnownHosts")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                upsert_known_host_from_scan(state, &profile, &scanned, &decision)?;
            }
            Ok(accept)
        }
        _ => Ok(false),
    }
}

async fn ensure_direct_ssh_host_key_trusted(
    state: &AppState,
    window: &tauri::Window,
    profile: &mut SshProfile,
) -> Result<(), String> {
    let store = read_store(state)?;
    let known_hosts = store
        .get("knownHosts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(public_key) =
        trusted_known_host_public_key(&known_hosts, &profile.address, profile.port)
    {
        profile.known_hosts_path =
            write_connection_known_hosts_from_public_key(state, profile, &public_key)?;
        return Ok(());
    }
    let scanned = scan_ssh_host_key(state, window, profile).await?;
    let decision =
        classify_scanned_host_key(&known_hosts, &profile.address, profile.port, &scanned);
    match decision
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
    {
        "trusted" => {
            profile.known_hosts_path = write_connection_known_hosts(state, profile, &scanned)?;
            Ok(())
        }
        "unknown" | "changed" => {
            let response =
                request_host_key_decision(state, window, profile, &scanned, &decision).await?;
            if !response
                .get("accept")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Err("已取消 SSH 主机密钥确认。".to_string());
            }
            if response
                .get("addToKnownHosts")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                upsert_known_host_from_scan(state, profile, &scanned, &decision)?;
            }
            profile.known_hosts_path = write_connection_known_hosts(state, profile, &scanned)?;
            Ok(())
        }
        _ => Err("SSH 主机密钥校验失败。".to_string()),
    }
}

async fn request_host_key_decision(
    state: &AppState,
    window: &tauri::Window,
    profile: &SshProfile,
    scanned: &Value,
    decision: &Value,
) -> Result<Value, String> {
    let request_id = random_id("hostkey");
    let (sender, receiver) = oneshot::channel();
    state
        .host_key_responses
        .lock()
        .map_err(error_string)?
        .insert(request_id.clone(), sender);
    let payload = json!({
        "requestId": request_id,
        "hostname": profile.address,
        "port": profile.port,
        "username": profile.username,
        "status": decision.get("status").and_then(Value::as_str).unwrap_or("unknown"),
        "keyType": scanned.get("keyType").and_then(Value::as_str).unwrap_or("unknown"),
        "fingerprint": scanned.get("fingerprint").and_then(Value::as_str).unwrap_or(""),
        "publicKey": scanned.get("publicKey").and_then(Value::as_str).unwrap_or(""),
        "knownHostId": decision.get("knownHostId").and_then(Value::as_str).unwrap_or(""),
        "knownFingerprint": decision.get("expectedFingerprint").and_then(Value::as_str).unwrap_or("")
    });
    if let Err(error) = window.emit("connection:host-key-verification", payload) {
        let _ = state
            .host_key_responses
            .lock()
            .map_err(error_string)?
            .remove(&request_id);
        return Err(error_string(error));
    }
    match time::timeout(Duration::from_secs(120), receiver).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(_)) => Err("主机密钥确认请求已关闭。".to_string()),
        Err(_) => {
            let _ = state
                .host_key_responses
                .lock()
                .map_err(error_string)?
                .remove(&request_id);
            Err("主机密钥确认超时。".to_string())
        }
    }
}

async fn scan_ssh_host_key(
    state: &AppState,
    window: &tauri::Window,
    profile: &SshProfile,
) -> Result<Value, String> {
    if let Some(jump) = profile.jump.as_deref() {
        return scan_ssh_host_key_via_jump(state, window, jump, profile).await;
    }
    if !command_exists("ssh-keyscan") {
        return Err("未找到 ssh-keyscan，无法执行 SSH 主机密钥预检。".to_string());
    }
    let mut command = Command::new("ssh-keyscan");
    prevent_tokio_process_window(&mut command);
    let output = command
        .args([
            "-T",
            "3",
            "-t",
            "ed25519,ecdsa,rsa",
            "-p",
            &profile.port.to_string(),
            &profile.address,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(error_string)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(public_key) = stdout.lines().find_map(parse_ssh_keyscan_line) else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "未能扫描 SSH 主机密钥。".to_string()
        } else {
            format!("未能扫描 SSH 主机密钥：{stderr}")
        });
    };
    scanned_host_key_from_public_key(&public_key)
}

async fn scan_ssh_host_key_via_jump(
    state: &AppState,
    window: &tauri::Window,
    jump: &SshProfile,
    profile: &SshProfile,
) -> Result<Value, String> {
    let command = format!(
        "ssh-keyscan -T 3 -t ed25519,ecdsa,rsa -p {} {}",
        profile.port,
        shell_quote(&profile.address)
    );
    let output = run_ssh_command_for_profile_with_window(
        state,
        Some(window.clone()),
        jump.clone(),
        command,
        String::new(),
    )
    .await?;
    let stdout = output.get("stdout").and_then(Value::as_str).unwrap_or("");
    let Some(public_key) = stdout.lines().find_map(parse_ssh_keyscan_line) else {
        let stderr = output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        return Err(if stderr.is_empty() {
            "未能通过跳板机读取目标主机 SSH 公钥。".to_string()
        } else {
            format!("未能通过跳板机读取目标主机 SSH 公钥：{stderr}")
        });
    };
    scanned_host_key_from_public_key(&public_key)
}

fn scanned_host_key_from_public_key(public_key: &str) -> Result<Value, String> {
    let public_key = public_key.trim();
    if public_key.is_empty() {
        return Err("SSH 主机公钥为空。".to_string());
    }
    let key_type = public_key.split_whitespace().next().unwrap_or("unknown");
    let fingerprint = fingerprint_from_public_key(public_key)?;
    Ok(json!({
        "keyType": key_type,
        "publicKey": public_key,
        "fingerprint": fingerprint
    }))
}

fn parse_ssh_keyscan_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }
    if !parts[1].starts_with("ssh-")
        && !parts[1].starts_with("ecdsa-")
        && !parts[1].starts_with("sk-")
    {
        return None;
    }
    Some(format!("{} {}", parts[1], parts[2]))
}

fn normalize_fingerprint(value: &str) -> String {
    value
        .trim()
        .strip_prefix("SHA256:")
        .unwrap_or(value.trim())
        .trim_end_matches('=')
        .to_string()
}

fn normalize_hostname(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn parse_known_host_pattern(hostname: &str) -> (String, Option<u16>) {
    let first_pattern = hostname.trim().split(',').next().unwrap_or("").trim();
    if first_pattern.is_empty() {
        return (String::new(), None);
    }
    if let Some(rest) = first_pattern.strip_prefix('[') {
        if let Some((host, port_text)) = rest.split_once("]:") {
            return (normalize_hostname(host), port_text.parse::<u16>().ok());
        }
    }
    (normalize_hostname(first_pattern), None)
}

fn known_host_port(known_host: &Value) -> u16 {
    let (_, parsed_port) = known_host
        .get("hostname")
        .and_then(Value::as_str)
        .map(parse_known_host_pattern)
        .unwrap_or_else(|| (String::new(), None));
    known_host
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .or(parsed_port)
        .unwrap_or(22)
}

fn fingerprint_from_public_key(public_key: &str) -> Result<String, String> {
    let parts = public_key.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err("SSH 主机公钥格式无效。".to_string());
    }
    let key_blob = base64::engine::general_purpose::STANDARD
        .decode(parts[1])
        .map_err(|_| "SSH 主机公钥格式无效。".to_string())?;
    Ok(base64::engine::general_purpose::STANDARD
        .encode(Sha256::digest(&key_blob))
        .trim_end_matches('=')
        .to_string())
}

pub(super) fn known_host_matches_host(known_host: &Value, hostname: &str, port: u16) -> bool {
    let (known_hostname, _) = known_host
        .get("hostname")
        .and_then(Value::as_str)
        .map(parse_known_host_pattern)
        .unwrap_or_else(|| (String::new(), None));
    if known_hostname.is_empty() || known_hostname.starts_with("|1|") {
        return false;
    }
    known_hostname == normalize_hostname(hostname) && known_host_port(known_host) == port
}

fn known_host_fingerprint(known_host: &Value) -> String {
    let fingerprint = known_host
        .get("fingerprint")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !fingerprint.trim().is_empty() {
        return normalize_fingerprint(fingerprint);
    }
    known_host
        .get("publicKey")
        .and_then(Value::as_str)
        .and_then(|public_key| fingerprint_from_public_key(public_key).ok())
        .unwrap_or_default()
}

pub(super) fn trusted_known_host_public_key(
    known_hosts: &[Value],
    hostname: &str,
    port: u16,
) -> Option<String> {
    known_hosts
        .iter()
        .filter(|known_host| known_host_matches_host(known_host, hostname, port))
        .find_map(|known_host| {
            known_host
                .get("publicKey")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|public_key| !public_key.is_empty())
                .map(ToOwned::to_owned)
        })
}

pub(super) fn classify_scanned_host_key(
    known_hosts: &[Value],
    hostname: &str,
    port: u16,
    scanned: &Value,
) -> Value {
    let scanned_fingerprint = normalize_fingerprint(
        scanned
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let key_type = scanned.get("keyType").and_then(Value::as_str).unwrap_or("");
    let candidates = known_hosts
        .iter()
        .filter(|known_host| known_host_matches_host(known_host, hostname, port))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return json!({ "status": "unknown" });
    }
    for known_host in &candidates {
        if known_host_fingerprint(known_host) == scanned_fingerprint {
            return json!({
                "status": "trusted",
                "knownHostId": known_host.get("id").and_then(Value::as_str).unwrap_or("")
            });
        }
    }
    for known_host in &candidates {
        if !key_type.is_empty()
            && key_type != "unknown"
            && known_host.get("keyType").and_then(Value::as_str) == Some(key_type)
        {
            return json!({
                "status": "changed",
                "knownHostId": known_host.get("id").and_then(Value::as_str).unwrap_or(""),
                "expectedFingerprint": known_host_fingerprint(known_host)
            });
        }
    }
    json!({ "status": "unknown" })
}

fn upsert_known_host_from_scan(
    state: &AppState,
    profile: &SshProfile,
    scanned: &Value,
    decision: &Value,
) -> Result<(), String> {
    let mut store = read_store(state)?;
    let current = store
        .get("knownHosts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let known_host_id = decision
        .get("knownHostId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let now_value = now();
    let mut replaced = false;
    let mut next = Vec::new();
    for known_host in current {
        let same_id = !known_host_id.is_empty()
            && known_host.get("id").and_then(Value::as_str) == Some(known_host_id);
        let same_host = known_host_matches_host(&known_host, &profile.address, profile.port);
        let same_fingerprint = same_host
            && known_host_fingerprint(&known_host)
                == normalize_fingerprint(
                    scanned
                        .get("fingerprint")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                );
        let same_host_type = same_host
            && !scanned
                .get("keyType")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty()
            && known_host.get("keyType").and_then(Value::as_str)
                == scanned.get("keyType").and_then(Value::as_str);
        if same_id || same_fingerprint || same_host_type {
            let next_known_host = json!({
                "id": known_host.get("id").and_then(Value::as_str).unwrap_or_else(|| if known_host_id.is_empty() { "" } else { known_host_id }),
                "hostname": profile.address,
                "port": profile.port,
                "keyType": scanned.get("keyType").and_then(Value::as_str).unwrap_or("unknown"),
                "publicKey": scanned.get("publicKey").and_then(Value::as_str).unwrap_or(""),
                "fingerprint": scanned.get("fingerprint").and_then(Value::as_str).unwrap_or(""),
                "discoveredAt": known_host.get("discoveredAt").and_then(Value::as_str).unwrap_or(&now_value),
                "lastSeen": now_value,
                "convertedToHostId": known_host.get("convertedToHostId").and_then(Value::as_str).unwrap_or("")
            });
            next.push(next_known_host.clone());
            replaced = true;
        } else {
            next.push(known_host);
        }
    }
    if !replaced {
        next.insert(
            0,
            json!({
                "id": if known_host_id.is_empty() { random_id("known-host") } else { known_host_id.to_string() },
                "hostname": profile.address,
                "port": profile.port,
                "keyType": scanned.get("keyType").and_then(Value::as_str).unwrap_or("unknown"),
                "publicKey": scanned.get("publicKey").and_then(Value::as_str).unwrap_or(""),
                "fingerprint": scanned.get("fingerprint").and_then(Value::as_str).unwrap_or(""),
                "discoveredAt": now_value,
                "lastSeen": now_value,
                "convertedToHostId": ""
            }),
        );
    }
    store["knownHosts"] = json!(next);
    write_store(state, &store)
}

fn write_connection_known_hosts(
    state: &AppState,
    profile: &SshProfile,
    scanned: &Value,
) -> Result<String, String> {
    let public_key = scanned
        .get("publicKey")
        .and_then(Value::as_str)
        .ok_or_else(|| "SSH 主机公钥为空。".to_string())?;
    write_connection_known_hosts_from_public_key(state, profile, public_key)
}

fn write_connection_known_hosts_from_public_key(
    state: &AppState,
    profile: &SshProfile,
    public_key: &str,
) -> Result<String, String> {
    let public_key = public_key.trim();
    if public_key.is_empty() {
        return Err("SSH 主机公钥为空。".to_string());
    }
    let known_hosts_dir = state.data_dir.join("known-hosts");
    fs::create_dir_all(&known_hosts_dir).map_err(error_string)?;
    let path = known_hosts_dir.join(format!("{}.known_hosts", random_id("ssh")));
    let host_pattern = known_hosts_host_pattern(&profile.address, profile.port);
    fs::write(&path, format!("{host_pattern} {public_key}\n")).map_err(error_string)?;
    Ok(path.to_string_lossy().to_string())
}

pub(super) fn known_hosts_host_pattern(address: &str, port: u16) -> String {
    if port == 22 {
        address.to_string()
    } else {
        format!("[{address}]:{port}")
    }
}

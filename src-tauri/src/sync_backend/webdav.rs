use serde_json::Value;
use std::time::Duration;

use crate::{error_string, random_id};

pub(super) fn normalize_webdav_url(value: &str, required: bool) -> Result<String, String> {
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

pub(super) async fn webdav_request(
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

pub(super) fn normalize_webdav_remote_path(value: &str) -> Result<String, String> {
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

pub(super) async fn ensure_webdav_directories(
    config: &Value,
    secrets: &Value,
) -> Result<(), String> {
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

pub(super) fn webdav_test_path(config: &Value) -> String {
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

pub(super) async fn webdav_response_error(response: reqwest::Response, action: &str) -> String {
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

pub(super) fn webdav_write_precondition_headers(
    etag: &str,
    exists: bool,
) -> Vec<(&'static str, String)> {
    if !etag.is_empty() {
        vec![("If-Match", etag.to_string())]
    } else if !exists {
        vec![("If-None-Match", "*".to_string())]
    } else {
        Vec::new()
    }
}

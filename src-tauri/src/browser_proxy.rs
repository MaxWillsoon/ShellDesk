use crate::{
    error_string, get_connection, https_url_origin, pick_free_local_port, start_ssh_local_forward,
    string_arg, wait_for_tcp, AppState, ConnectionKind,
};
use serde_json::{json, Value};
use std::{net::SocketAddr, process::Child as StdChild, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    time,
};

pub(crate) struct BrowserProxySession {
    pub(crate) connection_id: String,
    pub(crate) local_port: u16,
    pub(crate) shutdown: Option<oneshot::Sender<()>>,
    pub(crate) ssh_forward: Option<StdChild>,
}

pub(crate) async fn browser_resolve_url(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let raw_url = string_arg(&args, 1)?;
    let connection = get_connection(state, &connection_id)?;
    let parsed = reqwest::Url::parse(&raw_url).map_err(|_| "浏览器 URL 无效。".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("远程浏览器只支持 http 和 https URL。".to_string());
    }
    let trusted_certificate_origin = https_url_origin(parsed.as_str())
        .filter(|origin| connection.browser_certificate_trust.contains(origin));
    let trusted_certificate = trusted_certificate_origin.is_some();
    let use_trusted_https_proxy = parsed.scheme() == "https" && trusted_certificate;

    if connection.kind == ConnectionKind::Local {
        if use_trusted_https_proxy {
            let key = browser_proxy_key(
                &connection_id,
                "trusted-https",
                parsed.host_str().unwrap_or(""),
                parsed.port_or_known_default().unwrap_or(443),
            );
            let proxy_port = ensure_browser_reverse_proxy(
                state,
                key,
                connection_id.clone(),
                parsed.clone(),
                None,
                true,
            )
            .await?;
            let browser_url = browser_proxy_url(&parsed, proxy_port, "http")?;
            return Ok(json!({
                "url": parsed.to_string(),
                "browserUrl": browser_url,
                "proxied": true,
                "mode": "trusted-https-proxy",
                "localPort": proxy_port,
                "targetHost": parsed.host_str().unwrap_or(""),
                "targetPort": parsed.port_or_known_default().unwrap_or(443),
                "trustedCertificate": trusted_certificate,
                "trustedCertificateOrigin": trusted_certificate_origin
            }));
        }
        return Ok(json!({
            "url": parsed.to_string(),
            "browserUrl": parsed.to_string(),
            "proxied": false,
            "mode": "direct",
            "trustedCertificate": trusted_certificate,
            "trustedCertificateOrigin": trusted_certificate_origin
        }));
    }

    let target_host = parsed
        .host_str()
        .ok_or_else(|| "浏览器 URL 缺少主机名。".to_string())?
        .to_string();
    let target_port = parsed
        .port_or_known_default()
        .ok_or_else(|| "浏览器 URL 缺少端口。".to_string())?;
    let proxy_key_scheme = if use_trusted_https_proxy {
        "trusted-https"
    } else if parsed.scheme() == "https" {
        "reverse-https"
    } else {
        "reverse-http"
    };
    let key = browser_proxy_key(&connection_id, proxy_key_scheme, &target_host, target_port);

    if let Some(existing) = state
        .browser_proxies
        .lock()
        .map_err(error_string)?
        .get(&key)
    {
        let browser_url = browser_proxy_url(&parsed, existing.local_port, "http")?;
        return Ok(json!({
            "url": parsed.to_string(),
            "browserUrl": browser_url,
            "proxied": true,
            "mode": if use_trusted_https_proxy { "trusted-https-proxy" } else { "browser-reverse-proxy" },
            "localPort": existing.local_port,
            "targetHost": target_host,
            "targetPort": target_port,
            "trustedCertificate": trusted_certificate,
            "trustedCertificateOrigin": trusted_certificate_origin
        }));
    }

    let profile = connection
        .ssh
        .clone()
        .ok_or_else(|| "SSH profile is unavailable.".to_string())?;
    let local_port = pick_free_local_port()?;
    let mut child = start_ssh_local_forward(&profile, local_port, &target_host, target_port)?;
    if let Err(error) = wait_for_tcp("127.0.0.1", local_port, Duration::from_secs(8)).await {
        let _ = child.kill();
        return Err(format!("远程浏览器 SSH 转发启动失败：{error}"));
    }

    let proxy_port =
        match start_browser_reverse_proxy(parsed.clone(), Some(local_port), trusted_certificate)
            .await
        {
            Ok(proxy_port) => proxy_port,
            Err(error) => {
                let _ = child.kill();
                return Err(error);
            }
        };
    let browser_port = proxy_port.0;
    let browser_url = browser_proxy_url(&parsed, browser_port, "http")?;
    let shutdown = Some(proxy_port.1);
    state.browser_proxies.lock().map_err(error_string)?.insert(
        key,
        BrowserProxySession {
            connection_id: connection_id.clone(),
            local_port: browser_port,
            shutdown,
            ssh_forward: Some(child),
        },
    );

    Ok(json!({
        "url": parsed.to_string(),
        "browserUrl": browser_url,
        "proxied": true,
        "mode": if use_trusted_https_proxy { "trusted-https-proxy" } else { "browser-reverse-proxy" },
        "localPort": browser_port,
        "targetHost": target_host,
        "targetPort": target_port,
        "trustedCertificate": trusted_certificate,
        "trustedCertificateOrigin": trusted_certificate_origin
    }))
}

fn browser_proxy_key(connection_id: &str, scheme: &str, host: &str, port: u16) -> String {
    format!(
        "{connection_id}:{scheme}:{}:{port}",
        host.to_ascii_lowercase()
    )
}

fn browser_proxy_url(
    parsed: &reqwest::Url,
    local_port: u16,
    scheme: &str,
) -> Result<String, String> {
    let mut browser_url = parsed.clone();
    browser_url
        .set_scheme(scheme)
        .map_err(|_| "浏览器代理协议无效。".to_string())?;
    browser_url
        .set_host(Some("127.0.0.1"))
        .map_err(|_| "浏览器代理地址无效。".to_string())?;
    browser_url
        .set_port(Some(local_port))
        .map_err(|_| "浏览器代理端口无效。".to_string())?;
    Ok(browser_url.to_string())
}

async fn ensure_browser_reverse_proxy(
    state: &AppState,
    key: String,
    connection_id: String,
    upstream_url: reqwest::Url,
    upstream_forward_port: Option<u16>,
    accept_invalid_certs: bool,
) -> Result<u16, String> {
    if let Some(existing) = state
        .browser_proxies
        .lock()
        .map_err(error_string)?
        .get(&key)
    {
        return Ok(existing.local_port);
    }
    let (proxy_port, shutdown) =
        start_browser_reverse_proxy(upstream_url, upstream_forward_port, accept_invalid_certs)
            .await?;
    state.browser_proxies.lock().map_err(error_string)?.insert(
        key,
        BrowserProxySession {
            connection_id,
            local_port: proxy_port,
            shutdown: Some(shutdown),
            ssh_forward: None,
        },
    );
    Ok(proxy_port)
}

async fn start_browser_reverse_proxy(
    upstream_url: reqwest::Url,
    upstream_forward_port: Option<u16>,
    accept_invalid_certs: bool,
) -> Result<(u16, oneshot::Sender<()>), String> {
    let host = upstream_url
        .host_str()
        .ok_or_else(|| "浏览器 URL 缺少主机名。".to_string())?
        .to_string();
    let upstream_origin = format!(
        "{}://{}{}",
        upstream_url.scheme(),
        host,
        upstream_url
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default()
    );
    let mut client_builder = reqwest::Client::builder()
        .danger_accept_invalid_certs(accept_invalid_certs)
        .redirect(reqwest::redirect::Policy::none());
    if let Some(forward_port) = upstream_forward_port {
        client_builder = client_builder
            .resolve_to_addrs(&host, &[SocketAddr::from(([127, 0, 0, 1], forward_port))]);
    }
    let client = client_builder.build().map_err(error_string)?;
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(error_string)?;
    let proxy_port = listener.local_addr().map_err(error_string)?.port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            let next_client = client.clone();
                            let next_origin = upstream_origin.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = handle_trusted_https_browser_proxy(stream, next_client, next_origin).await;
                            });
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });
    Ok((proxy_port, shutdown_tx))
}

async fn handle_trusted_https_browser_proxy(
    mut stream: TcpStream,
    client: reqwest::Client,
    upstream_origin: String,
) -> Result<(), String> {
    let (method, target, headers, body) = read_browser_http_request(&mut stream).await?;
    let browser_host = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let upstream_url = if target.starts_with("http://") || target.starts_with("https://") {
        target
    } else if target.starts_with('/') {
        format!("{upstream_origin}{target}")
    } else {
        format!("{upstream_origin}/{target}")
    };
    let method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| "浏览器请求方法无效。".to_string())?;
    let mut request = client.request(method, upstream_url);
    for (name, value) in headers {
        let name_lower = name.to_ascii_lowercase();
        if matches!(
            name_lower.as_str(),
            "connection"
                | "proxy-connection"
                | "host"
                | "keep-alive"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
        ) {
            continue;
        }
        request = request.header(name, value);
    }
    if !body.is_empty() {
        request = request.body(body);
    }
    let response = request.send().await.map_err(error_string)?;
    let status = response.status();
    let response_headers = response.headers().clone();
    let body = response.bytes().await.map_err(error_string)?;
    let reason = status.canonical_reason().unwrap_or("");
    let mut raw = format!("HTTP/1.1 {} {}\r\n", status.as_u16(), reason);
    for (name, value) in response_headers.iter() {
        let name_lower = name.as_str().to_ascii_lowercase();
        if matches!(
            name_lower.as_str(),
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | "content-length"
        ) {
            continue;
        }
        if let Ok(mut value) = value.to_str().map(str::to_string) {
            if name_lower == "location"
                && value.starts_with(&upstream_origin)
                && !browser_host.is_empty()
            {
                value = format!("http://{}{}", browser_host, &value[upstream_origin.len()..]);
            }
            raw.push_str(name.as_str());
            raw.push_str(": ");
            raw.push_str(&value);
            raw.push_str("\r\n");
        }
    }
    raw.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    stream
        .write_all(raw.as_bytes())
        .await
        .map_err(error_string)?;
    stream.write_all(&body).await.map_err(error_string)?;
    let _ = stream.shutdown().await;
    Ok(())
}

async fn read_browser_http_request(
    stream: &mut TcpStream,
) -> Result<(String, String, Vec<(String, String)>, Vec<u8>), String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let read = time::timeout(Duration::from_secs(15), stream.read(&mut chunk))
            .await
            .map_err(|_| "浏览器请求读取超时。".to_string())?
            .map_err(error_string)?;
        if read == 0 {
            return Err("浏览器请求为空。".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > 128 * 1024 {
            return Err("浏览器请求头过大。".to_string());
        }
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "浏览器请求行为空。".to_string())?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| "浏览器请求方法为空。".to_string())?
        .to_string();
    let target = request_parts
        .next()
        .ok_or_else(|| "浏览器请求目标为空。".to_string())?
        .to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse::<usize>().unwrap_or(0).min(16 * 1024 * 1024);
        }
        headers.push((name, value));
    }
    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let read = time::timeout(Duration::from_secs(15), stream.read(&mut chunk))
            .await
            .map_err(|_| "浏览器请求体读取超时。".to_string())?
            .map_err(error_string)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);
    Ok((method, target, headers, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_proxy_url_serves_https_targets_over_local_http() {
        let parsed = reqwest::Url::parse("https://Example.COM:8443/a/b?q=1#frag").unwrap();

        let browser_url = browser_proxy_url(&parsed, 32123, "http").unwrap();

        assert_eq!(browser_url, "http://127.0.0.1:32123/a/b?q=1#frag");
    }

    #[test]
    fn browser_proxy_keys_distinguish_plain_and_trusted_https() {
        let plain = browser_proxy_key("conn-1", "reverse-https", "Example.COM", 443);
        let trusted = browser_proxy_key("conn-1", "trusted-https", "example.com", 443);

        assert_ne!(plain, trusted);
        assert_eq!(plain, "conn-1:reverse-https:example.com:443");
    }
}

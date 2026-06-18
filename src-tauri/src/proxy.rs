use base64::Engine;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    process::Stdio,
    thread,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    time,
};

use crate::{error_string, now, prevent_tokio_process_window, read_string_field};

#[derive(Clone)]
pub(crate) struct SshProxyConfig {
    pub(crate) proxy_type: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) command: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) helper_id: String,
}

pub(crate) fn proxy_helper_env_name(helper_id: &str) -> String {
    let suffix = helper_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("SHELLDESK_PROXY_CONFIG_{suffix}")
}

pub(crate) fn run_proxy_helper_from_args() -> Option<i32> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) != Some("--shelldesk-proxy-helper") {
        return None;
    }
    let result = if args.len() < 5 {
        Err("Proxy helper 参数不足。".to_string())
    } else {
        let helper_id = args[2].clone();
        let target_host = args[3].clone();
        let target_port = args[4]
            .parse::<u16>()
            .map_err(|_| "Proxy helper 目标端口无效。".to_string());
        match target_port {
            Ok(target_port) => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(error_string)
                .and_then(|runtime| {
                    runtime.block_on(run_proxy_helper(helper_id, target_host, target_port))
                }),
            Err(error) => Err(error),
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        return Some(1);
    }
    Some(0)
}

async fn run_proxy_helper(
    helper_id: String,
    target_host: String,
    target_port: u16,
) -> Result<(), String> {
    let env_name = proxy_helper_env_name(&helper_id);
    let encoded =
        std::env::var(&env_name).map_err(|_| "Proxy helper 缺少代理配置。".to_string())?;
    let config_bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Proxy helper 代理配置格式无效。".to_string())?;
    let config: Value = serde_json::from_slice(&config_bytes)
        .map_err(|_| "Proxy helper 代理配置格式无效。".to_string())?;
    let proxy_type = read_string_field(&config, "type", "");
    let timeout = Duration::from_secs(30);
    let stream = match proxy_type.as_str() {
        "http" => open_http_proxy_tunnel(&config, &target_host, target_port, timeout).await?,
        "socks5" => open_socks5_proxy_tunnel(&config, &target_host, target_port, timeout).await?,
        _ => return Err("Proxy helper 不支持该代理类型。".to_string()),
    };
    proxy_helper_copy_stdio(stream)
}

fn proxy_helper_copy_stdio(stream: TcpStream) -> Result<(), String> {
    let stream = stream.into_std().map_err(error_string)?;
    stream.set_nonblocking(false).map_err(error_string)?;
    let mut remote_reader = stream.try_clone().map_err(error_string)?;
    let mut remote_writer = stream;
    let client_to_remote = thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let result = std::io::copy(&mut stdin, &mut remote_writer).map_err(error_string);
        let _ = remote_writer.shutdown(std::net::Shutdown::Write);
        result.map(|_| ())
    });
    let remote_to_client = thread::spawn(move || {
        let mut stdout = std::io::stdout().lock();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let read = remote_reader.read(&mut buffer).map_err(error_string)?;
            if read == 0 {
                return stdout.flush().map_err(error_string);
            }
            stdout.write_all(&buffer[..read]).map_err(error_string)?;
            stdout.flush().map_err(error_string)?;
        }
    });
    client_to_remote
        .join()
        .map_err(|_| "Proxy helper 输入转发线程已崩溃。".to_string())??;
    remote_to_client
        .join()
        .map_err(|_| "Proxy helper 输出转发线程已崩溃。".to_string())??;
    Ok(())
}

pub(crate) async fn test_proxy(args: Vec<Value>) -> Result<Value, String> {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let request = read_proxy_test_request(&payload)?;
    let timeout = Duration::from_millis(request.timeout_ms);
    let started = std::time::Instant::now();
    let result = match request.proxy_type.as_str() {
        "http" => {
            test_http_proxy(
                &request.config,
                &request.target_host,
                request.target_port,
                &request.target_kind,
                timeout,
            )
            .await
        }
        "socks5" => {
            test_socks5_proxy(
                &request.config,
                &request.target_host,
                request.target_port,
                &request.target_kind,
                timeout,
            )
            .await
        }
        "command" => {
            test_command_proxy(
                &request.config,
                &request.target_host,
                request.target_port,
                &request.target_kind,
                timeout,
            )
            .await
        }
        _ => Err("代理类型无效。".to_string()),
    };
    Ok(json!({
        "ok": result.is_ok(),
        "targetHost": request.target_host,
        "targetPort": request.target_port,
        "latencyMs": started.elapsed().as_millis() as u64,
        "checkedAt": now(),
        "error": result.err().unwrap_or_default()
    }))
}

#[derive(Debug, PartialEq, Eq)]
struct ProxyTestRequest {
    proxy_type: String,
    config: Value,
    target_kind: String,
    target_host: String,
    target_port: u16,
    timeout_ms: u64,
}

fn read_proxy_test_request(raw_payload: &Value) -> Result<ProxyTestRequest, String> {
    let config_value = raw_payload
        .as_object()
        .and_then(|payload| payload.get("config"))
        .unwrap_or(raw_payload);
    let target_value = raw_payload
        .as_object()
        .and_then(|payload| payload.get("target"))
        .unwrap_or(&Value::Null);
    let config = read_proxy_test_config(config_value)?;
    let target = read_proxy_test_target(target_value)?;
    Ok(ProxyTestRequest {
        proxy_type: read_string_field(&config, "type", ""),
        config,
        target_kind: target.target_kind,
        target_host: target.target_host,
        target_port: target.target_port,
        timeout_ms: target.timeout_ms,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct ProxyTestTarget {
    target_kind: String,
    target_host: String,
    target_port: u16,
    timeout_ms: u64,
}

fn read_proxy_test_config(raw_config: &Value) -> Result<Value, String> {
    let Some(config) = raw_config.as_object() else {
        return Err("代理配置无效。".to_string());
    };
    let proxy_type = config
        .get("type")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "http" | "socks5" | "command"))
        .ok_or_else(|| "代理类型无效。".to_string())?;
    if proxy_type == "command" {
        return Ok(json!({
            "type": proxy_type,
            "host": "",
            "port": 0,
            "command": read_bounded_string(config.get("command"), "代理命令", 4096, true, true)?,
            "username": "",
            "password": ""
        }));
    }
    Ok(json!({
        "type": proxy_type,
        "host": read_bounded_string(config.get("host"), "代理主机", 255, true, true)?,
        "port": read_integer_in_range(config.get("port"), "代理端口", 1, 65535)?,
        "command": "",
        "username": read_bounded_string(config.get("username"), "代理用户名", 128, false, true)?,
        "password": read_bounded_string(config.get("password"), "代理密码", 4096, false, false)?
    }))
}

fn read_proxy_test_target(raw_target: &Value) -> Result<ProxyTestTarget, String> {
    let Some(target) = raw_target.as_object() else {
        return Ok(ProxyTestTarget {
            target_kind: "http".to_string(),
            target_host: "example.com".to_string(),
            target_port: 80,
            timeout_ms: 15000,
        });
    };
    let target_kind = if target.get("kind").and_then(Value::as_str) == Some("ssh") {
        "ssh"
    } else {
        "http"
    };
    let default_port = if target_kind == "ssh" { 22 } else { 80 };
    Ok(ProxyTestTarget {
        target_kind: target_kind.to_string(),
        target_host: read_bounded_string_with_default(
            target.get("host"),
            "example.com",
            "代理测试目标主机",
            255,
        )?,
        target_port: read_integer_in_range_with_default(
            target.get("port"),
            default_port,
            "代理测试目标端口",
            1,
            65535,
        )?,
        timeout_ms: read_integer_in_range_with_default(
            target.get("timeoutMs"),
            15000,
            "代理测试超时时间",
            3000,
            30000,
        )? as u64,
    })
}

fn read_bounded_string(
    value: Option<&Value>,
    label: &str,
    max_length: usize,
    required: bool,
    trim: bool,
) -> Result<String, String> {
    let value = match value {
        Some(Value::String(value)) => value.as_str(),
        Some(Value::Null) | None => "",
        Some(_) => return Err(format!("{label}无效。")),
    };
    read_bounded_string_inner(value, label, max_length, required, trim)
}

fn read_integer_in_range(
    value: Option<&Value>,
    label: &str,
    min_value: u16,
    max_value: u16,
) -> Result<u16, String> {
    let Some(value) = value else {
        return Err(format!("{label}无效。"));
    };
    let parsed = match value {
        Value::Number(number) => number.as_i64(),
        Value::String(value) => value.trim().parse::<i64>().ok(),
        _ => None,
    };
    let Some(parsed) = parsed else {
        return Err(format!("{label}无效。"));
    };
    if parsed < i64::from(min_value) || parsed > i64::from(max_value) {
        return Err(format!("{label}无效。"));
    }
    Ok(parsed as u16)
}

fn read_bounded_string_with_default(
    value: Option<&Value>,
    fallback: &str,
    label: &str,
    max_length: usize,
) -> Result<String, String> {
    let value = match value {
        Some(Value::String(value)) => value.as_str(),
        Some(Value::Null) | None => fallback,
        Some(_) => return Err(format!("{label}无效。")),
    };
    read_bounded_string_inner(value, label, max_length, true, true)
}

fn read_integer_in_range_with_default(
    value: Option<&Value>,
    fallback: u16,
    label: &str,
    min_value: u16,
    max_value: u16,
) -> Result<u16, String> {
    match value {
        Some(Value::Null) | None => {
            read_integer_in_range(Some(&json!(fallback)), label, min_value, max_value)
        }
        Some(value) => read_integer_in_range(Some(value), label, min_value, max_value),
    }
}

fn read_bounded_string_inner(
    value: &str,
    label: &str,
    max_length: usize,
    required: bool,
    trim: bool,
) -> Result<String, String> {
    let next_value = if trim {
        value.trim().to_string()
    } else {
        value.to_string()
    };
    if required && next_value.is_empty() {
        return Err(format!("请输入{label}。"));
    }
    if next_value.chars().count() > max_length
        || next_value.contains('\0')
        || next_value.contains(['\r', '\n'])
    {
        return Err(format!("{label}无效。"));
    }
    Ok(next_value)
}

async fn test_http_proxy(
    config: &Value,
    target_host: &str,
    target_port: u16,
    target_kind: &str,
    timeout: Duration,
) -> Result<(), String> {
    let stream = open_http_proxy_tunnel(config, target_host, target_port, timeout).await?;
    let (mut reader, mut writer) = tokio::io::split(stream);
    test_proxy_target(&mut reader, &mut writer, target_kind, target_host, timeout).await
}

async fn open_http_proxy_tunnel(
    config: &Value,
    target_host: &str,
    target_port: u16,
    timeout: Duration,
) -> Result<TcpStream, String> {
    let proxy_host = read_string_field(config, "host", "").trim().to_string();
    let proxy_port = config
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "代理主机或端口为空。".to_string())?;
    if proxy_host.is_empty() {
        return Err("代理主机或端口为空。".to_string());
    }
    let mut stream =
        connect_tcp_with_timeout(&proxy_host, proxy_port, "HTTP 代理", timeout).await?;
    let username = read_string_field(config, "username", "");
    let password = read_string_field(config, "password", "");
    let auth_header = if username.is_empty() {
        String::new()
    } else {
        let auth =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        format!("Proxy-Authorization: Basic {auth}\r\n")
    };
    let request = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\nProxy-Connection: Keep-Alive\r\n{auth_header}\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(error_string)?;
    let header = read_http_header(&mut stream, timeout, "HTTP 代理响应头过大。").await?;
    let status_line = header.lines().next().unwrap_or("").trim().to_string();
    if !http_status_matches(&status_line, Some('2')) {
        return Err(format!(
            "HTTP 代理拒绝连接：{}",
            if status_line.is_empty() {
                "无状态行"
            } else {
                status_line.as_str()
            }
        ));
    }
    Ok(stream)
}

async fn test_socks5_proxy(
    config: &Value,
    target_host: &str,
    target_port: u16,
    target_kind: &str,
    timeout: Duration,
) -> Result<(), String> {
    let stream = open_socks5_proxy_tunnel(config, target_host, target_port, timeout).await?;
    let (mut reader, mut writer) = tokio::io::split(stream);
    test_proxy_target(&mut reader, &mut writer, target_kind, target_host, timeout).await
}

async fn open_socks5_proxy_tunnel(
    config: &Value,
    target_host: &str,
    target_port: u16,
    timeout: Duration,
) -> Result<TcpStream, String> {
    let proxy_host = read_string_field(config, "host", "").trim().to_string();
    let proxy_port = config
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "代理主机或端口为空。".to_string())?;
    if proxy_host.is_empty() {
        return Err("代理主机或端口为空。".to_string());
    }
    let mut stream =
        connect_tcp_with_timeout(&proxy_host, proxy_port, "SOCKS5 代理", timeout).await?;
    let username = read_string_field(config, "username", "");
    let password = read_string_field(config, "password", "");
    if username.is_empty() {
        stream
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .map_err(error_string)?;
    } else {
        stream
            .write_all(&[0x05, 0x02, 0x00, 0x02])
            .await
            .map_err(error_string)?;
    }
    let mut method = [0u8; 2];
    read_exact_with_timeout(&mut stream, &mut method, timeout).await?;
    if method[0] != 0x05 || method[1] == 0xff {
        return Err("SOCKS5 代理没有可用认证方式。".to_string());
    }
    if method[1] == 0x02 {
        let username_bytes = username.as_bytes();
        let password_bytes = password.as_bytes();
        if username_bytes.len() > 255 || password_bytes.len() > 255 {
            return Err("SOCKS5 代理用户名或密码过长。".to_string());
        }
        let mut auth = Vec::with_capacity(username_bytes.len() + password_bytes.len() + 3);
        auth.push(0x01);
        auth.push(username_bytes.len() as u8);
        auth.extend_from_slice(username_bytes);
        auth.push(password_bytes.len() as u8);
        auth.extend_from_slice(password_bytes);
        stream.write_all(&auth).await.map_err(error_string)?;
        let mut auth_response = [0u8; 2];
        read_exact_with_timeout(&mut stream, &mut auth_response, timeout).await?;
        if auth_response[1] != 0x00 {
            return Err("SOCKS5 代理认证失败。".to_string());
        }
    } else if method[1] != 0x00 {
        return Err("SOCKS5 代理返回了不支持的认证方式。".to_string());
    }

    let mut request = vec![0x05, 0x01, 0x00];
    request.extend_from_slice(&encode_socks5_address(target_host)?);
    request.extend_from_slice(&target_port.to_be_bytes());
    stream.write_all(&request).await.map_err(error_string)?;
    let mut response_head = [0u8; 4];
    read_exact_with_timeout(&mut stream, &mut response_head, timeout).await?;
    if response_head[0] != 0x05 || response_head[1] != 0x00 {
        return Err(format!(
            "SOCKS5 代理连接失败，响应码 {}。",
            response_head[1]
        ));
    }
    match response_head[3] {
        0x01 => {
            let mut skip = [0u8; 4];
            read_exact_with_timeout(&mut stream, &mut skip, timeout).await?;
        }
        0x03 => {
            let mut len = [0u8; 1];
            read_exact_with_timeout(&mut stream, &mut len, timeout).await?;
            let mut skip = vec![0u8; len[0] as usize];
            read_exact_with_timeout(&mut stream, &mut skip, timeout).await?;
        }
        0x04 => {
            let mut skip = [0u8; 16];
            read_exact_with_timeout(&mut stream, &mut skip, timeout).await?;
        }
        _ => return Err("SOCKS5 代理响应地址类型无效。".to_string()),
    }
    let mut bound_port = [0u8; 2];
    read_exact_with_timeout(&mut stream, &mut bound_port, timeout).await?;
    Ok(stream)
}

async fn test_command_proxy(
    config: &Value,
    target_host: &str,
    target_port: u16,
    target_kind: &str,
    timeout: Duration,
) -> Result<(), String> {
    let command_line = read_string_field(config, "command", "")
        .replace("{host}", target_host)
        .replace("%h", target_host)
        .replace("{port}", &target_port.to_string())
        .replace("%p", &target_port.to_string())
        .trim()
        .to_string();
    if command_line.is_empty() {
        return Err("ProxyCommand 不能为空。".to_string());
    }
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", &command_line]);
        command
    } else {
        let mut command = Command::new("sh");
        command.args(["-c", &command_line]);
        command
    };
    prevent_tokio_process_window(&mut command);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(error_string)?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "ProxyCommand 标准输入不可写。".to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ProxyCommand 标准输出不可读。".to_string())?;
    let result =
        test_proxy_target(&mut stdout, &mut stdin, target_kind, target_host, timeout).await;
    if result.is_err() {
        if let Some(mut stderr) = child.stderr.take() {
            let mut buffer = vec![0u8; 8192];
            if let Ok(Ok(count)) =
                time::timeout(Duration::from_millis(200), stderr.read(&mut buffer)).await
            {
                let stderr_text = String::from_utf8_lossy(&buffer[..count]).trim().to_string();
                if !stderr_text.is_empty() {
                    let _ = child.kill().await;
                    return Err(stderr_text);
                }
            }
        }
    }
    let _ = child.kill().await;
    result
}

async fn connect_tcp_with_timeout(
    host: &str,
    port: u16,
    label: &str,
    timeout: Duration,
) -> Result<TcpStream, String> {
    match time::timeout(timeout, TcpStream::connect((host, port))).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => Err(format!("{label}连接失败：{error}")),
        Err(_) => Err(format!("{label}连接超时。")),
    }
}

async fn read_exact_with_timeout<R: AsyncRead + Unpin>(
    reader: &mut R,
    buffer: &mut [u8],
    timeout: Duration,
) -> Result<(), String> {
    match time::timeout(timeout, reader.read_exact(buffer)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(error_string(error)),
        Err(_) => Err("代理测试超时。".to_string()),
    }
}

async fn read_http_header<R: AsyncRead + Unpin>(
    reader: &mut R,
    timeout: Duration,
    oversize_message: &str,
) -> Result<String, String> {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        read_exact_with_timeout(reader, &mut byte, timeout).await?;
        buffer.push(byte[0]);
        if buffer.len() > 64 * 1024 {
            return Err(oversize_message.to_string());
        }
        if buffer.ends_with(b"\r\n\r\n") || buffer.ends_with(b"\n\n") {
            return Ok(String::from_utf8_lossy(&buffer).to_string());
        }
    }
}

pub(crate) async fn test_proxy_target<R, W>(
    reader: &mut R,
    writer: &mut W,
    target_kind: &str,
    target_host: &str,
    timeout: Duration,
) -> Result<(), String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if target_kind == "ssh" {
        read_ssh_banner(reader, timeout).await?;
        return Ok(());
    }
    time::sleep(Duration::from_millis(40)).await;
    let request = format!("HEAD / HTTP/1.1\r\nHost: {target_host}\r\nConnection: close\r\n\r\n");
    writer
        .write_all(request.as_bytes())
        .await
        .map_err(error_string)?;
    let header = read_http_header(reader, timeout, "代理测试响应过大。").await?;
    let status_line = header.lines().next().unwrap_or("").trim();
    if !http_status_matches(status_line, None) {
        return Err("代理已连接，但测试目标响应不是有效 HTTP。".to_string());
    }
    Ok(())
}

async fn read_ssh_banner<R: AsyncRead + Unpin>(
    reader: &mut R,
    timeout: Duration,
) -> Result<String, String> {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        read_exact_with_timeout(reader, &mut byte, timeout).await?;
        buffer.push(byte[0]);
        let text = String::from_utf8_lossy(&buffer);
        if let Some(line) = text.lines().find(|line| line.starts_with("SSH-")) {
            return Ok(line.trim().to_string());
        }
        let trimmed = text.trim_start().to_ascii_lowercase();
        if trimmed.starts_with("http/")
            || trimmed.starts_with("<!doctype html")
            || trimmed.starts_with("<html")
        {
            return Err("代理已连接，但测试目标响应不是 SSH 服务。".to_string());
        }
        if buffer.len() > 8192 {
            return Err("代理已连接，但目标 SSH 服务未返回有效握手 banner。".to_string());
        }
    }
}

fn http_status_matches(status_line: &str, first_digit: Option<char>) -> bool {
    let mut parts = status_line.split_whitespace();
    let Some(protocol) = parts.next() else {
        return false;
    };
    let Some(status) = parts.next() else {
        return false;
    };
    if !protocol.to_ascii_uppercase().starts_with("HTTP/") {
        return false;
    }
    if status.len() != 3 || !status.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    first_digit
        .map(|digit| status.starts_with(digit))
        .unwrap_or(true)
}

fn encode_socks5_address(host: &str) -> Result<Vec<u8>, String> {
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        return Ok(match ip {
            std::net::IpAddr::V4(value) => {
                let mut output = vec![0x01];
                output.extend_from_slice(&value.octets());
                output
            }
            std::net::IpAddr::V6(value) => {
                let mut output = vec![0x04];
                output.extend_from_slice(&value.octets());
                output
            }
        });
    }
    let bytes = host.as_bytes();
    if bytes.len() > 255 {
        return Err("SOCKS5 目标主机名过长。".to_string());
    }
    let mut output = vec![0x03, bytes.len() as u8];
    output.extend_from_slice(bytes);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_test_request_accepts_legacy_raw_config_payload() {
        let request = read_proxy_test_request(&json!({
            "type": "http",
            "host": " proxy.local ",
            "port": "8080",
            "username": " user ",
            "password": " pass "
        }))
        .unwrap();

        assert_eq!(request.proxy_type, "http");
        assert_eq!(request.config["host"], "proxy.local");
        assert_eq!(request.config["port"], 8080);
        assert_eq!(request.config["username"], "user");
        assert_eq!(request.config["password"], " pass ");
        assert_eq!(request.target_kind, "http");
        assert_eq!(request.target_host, "example.com");
        assert_eq!(request.target_port, 80);
        assert_eq!(request.timeout_ms, 15000);
    }

    #[test]
    fn proxy_test_request_accepts_wrapped_config_and_target() {
        let request = read_proxy_test_request(&json!({
            "config": {
                "type": "socks5",
                "host": "127.0.0.1",
                "port": 1080
            },
            "target": {
                "kind": "ssh",
                "host": "server.internal",
                "port": "2222",
                "timeoutMs": "3000"
            }
        }))
        .unwrap();

        assert_eq!(request.proxy_type, "socks5");
        assert_eq!(request.target_kind, "ssh");
        assert_eq!(request.target_host, "server.internal");
        assert_eq!(request.target_port, 2222);
        assert_eq!(request.timeout_ms, 3000);
    }

    #[test]
    fn proxy_test_request_normalizes_command_proxy_config() {
        let request = read_proxy_test_request(&json!({
            "config": {
                "type": "command",
                "command": " nc -X connect -x proxy:8080 {host} {port} ",
                "host": "ignored",
                "port": 1,
                "username": "ignored",
                "password": "ignored"
            }
        }))
        .unwrap();

        assert_eq!(request.proxy_type, "command");
        assert_eq!(request.config["host"], "");
        assert_eq!(request.config["port"], 0);
        assert_eq!(
            request.config["command"],
            "nc -X connect -x proxy:8080 {host} {port}"
        );
        assert_eq!(request.config["username"], "");
        assert_eq!(request.config["password"], "");
    }

    #[test]
    fn proxy_test_request_matches_legacy_validation_errors() {
        assert_eq!(
            read_proxy_test_request(&Value::Null).unwrap_err(),
            "代理配置无效。"
        );
        assert_eq!(
            read_proxy_test_request(&json!({ "type": "bogus" })).unwrap_err(),
            "代理类型无效。"
        );
        assert_eq!(
            read_proxy_test_request(&json!({ "type": "http", "port": 8080 })).unwrap_err(),
            "请输入代理主机。"
        );
        assert_eq!(
            read_proxy_test_request(&json!({
                "config": { "type": "http", "host": "proxy.local", "port": 8080 },
                "target": { "host": "example.com", "timeoutMs": 100 }
            }))
            .unwrap_err(),
            "代理测试超时时间无效。"
        );
    }
}

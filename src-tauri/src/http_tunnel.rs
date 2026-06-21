use crate::{
    error_string, get_connection,
    ssh_tunnel::{config_from_connection_with_window, create_tunnel, SshTunnel, SshTunnelError},
    string_arg, AppState, ConnectionKind,
};
use reqwest::StatusCode;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use thiserror::Error;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const TUNNEL_IDLE_TTL: Duration = Duration::from_secs(60);

pub(crate) struct HttpTunnelClient {
    client: reqwest::Client,
    base_url: String,
    auth: Option<(String, String)>,
}

impl HttpTunnelClient {
    pub(crate) fn new(base_url: String, auth: Option<(String, String)>, ignore_ssl: bool) -> Self {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .danger_accept_invalid_certs(ignore_ssl)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth,
        }
    }

    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, HttpTunnelError> {
        self.send(self.client.get(self.url(path))).await
    }

    pub(crate) async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Value,
    ) -> Result<T, HttpTunnelError> {
        self.send(self.client.post(self.url(path)).json(&body))
            .await
    }

    pub(crate) async fn put<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Value,
    ) -> Result<T, HttpTunnelError> {
        self.send(self.client.put(self.url(path)).json(&body)).await
    }

    pub(crate) async fn delete<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, HttpTunnelError> {
        self.send(self.client.delete(self.url(path))).await
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        if path.starts_with('/') {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/{}", self.base_url, path)
        }
    }

    async fn send<T: DeserializeOwned>(
        &self,
        mut request: reqwest::RequestBuilder,
    ) -> Result<T, HttpTunnelError> {
        if let Some((username, password)) = &self.auth {
            request = request.basic_auth(username, Some(password));
        }

        let response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                HttpTunnelError::Timeout
            } else {
                HttpTunnelError::Http(error)
            }
        })?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(HttpTunnelError::AuthRequired);
        }
        let response = response.error_for_status()?;
        response.json::<T>().await.map_err(|error| {
            if error.is_timeout() {
                HttpTunnelError::Timeout
            } else if error.is_decode() {
                HttpTunnelError::Json(error.to_string())
            } else {
                HttpTunnelError::Http(error)
            }
        })
    }
}

#[derive(Debug, Error)]
pub(crate) enum HttpTunnelError {
    #[error(transparent)]
    Tunnel(#[from] SshTunnelError),
    #[error("HTTP 请求失败：{0}")]
    Http(#[from] reqwest::Error),
    #[error("HTTP 响应 JSON 解析失败：{0}")]
    Json(String),
    #[error("HTTP 请求超时。")]
    Timeout,
    #[error("HTTP 服务需要认证，请检查用户名和密码。")]
    AuthRequired,
}

impl HttpTunnelError {
    pub(crate) fn user_message(&self) -> String {
        self.to_string()
    }
}

pub(crate) struct HttpTunnelSession {
    pub(crate) connection_id: String,
    pub(crate) tunnel: SshTunnel,
    pub(crate) last_used: Instant,
    pub(crate) active_requests: usize,
}

impl HttpTunnelSession {
    pub(crate) async fn shutdown(self) -> Result<(), SshTunnelError> {
        self.tunnel.shutdown().await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpTunnelAuth {
    username: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpTunnelRequest {
    connection_id: String,
    target_host: String,
    target_port: u16,
    path: String,
    auth: Option<HttpTunnelAuth>,
    body: Option<Value>,
    #[serde(default)]
    ignore_ssl: bool,
    #[serde(default)]
    secure: Option<bool>,
}

pub(crate) async fn get(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    execute(state, window, "GET", args).await
}

pub(crate) async fn post(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    execute(state, window, "POST", args).await
}

pub(crate) async fn put(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    execute(state, window, "PUT", args).await
}

pub(crate) async fn delete(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    execute(state, window, "DELETE", args).await
}

async fn execute(
    state: &AppState,
    window: &tauri::Window,
    method: &str,
    args: Vec<Value>,
) -> Result<Value, String> {
    let request = parse_request(args)?;
    validate_target(&request.target_host, request.target_port)?;
    let local_port = acquire_tunnel(state, window, &request).await?;
    let scheme = if request.secure.unwrap_or(request.target_port == 443) {
        "https"
    } else {
        "http"
    };
    let client = HttpTunnelClient::new(
        format!("{scheme}://127.0.0.1:{local_port}"),
        request
            .auth
            .as_ref()
            .map(|auth| (auth.username.clone(), auth.password.clone())),
        request.ignore_ssl,
    );

    let result = match method {
        "GET" => client.get::<Value>(&request.path).await,
        "POST" => {
            client
                .post::<Value>(&request.path, request.body.clone().unwrap_or(Value::Null))
                .await
        }
        "PUT" => {
            client
                .put::<Value>(&request.path, request.body.clone().unwrap_or(Value::Null))
                .await
        }
        "DELETE" => client.delete::<Value>(&request.path).await,
        _ => unreachable!("unsupported HTTP method"),
    }
    .map_err(|error| error.user_message());

    release_tunnel(state, &request);
    schedule_idle_cleanup(state.clone(), session_key(&request));
    result
}

async fn acquire_tunnel(
    state: &AppState,
    window: &tauri::Window,
    request: &HttpTunnelRequest,
) -> Result<u16, String> {
    let key = session_key(request);
    if let Some(port) = try_reuse_tunnel(state, &key)? {
        return Ok(port);
    }

    let connection = get_connection(state, &request.connection_id)?;
    let local_kind = connection.kind == ConnectionKind::Local;
    drop(connection);
    if local_kind {
        return Err("本地连接暂不支持自动创建 HTTP SSH 隧道。".to_string());
    }

    let config = config_from_connection_with_window(
        state,
        window,
        &request.connection_id,
        &request.target_host,
        request.target_port,
        None,
    )
    .await?;
    let tunnel = create_tunnel(config)
        .await
        .map_err(|error| HttpTunnelError::Tunnel(error).user_message())?;
    let local_port = tunnel.local_addr().port();

    let existing_port = {
        let mut sessions = state.http_tunnel_sessions.lock().map_err(error_string)?;
        sessions.get_mut(&key).map(|session| {
            session.active_requests += 1;
            session.last_used = Instant::now();
            session.tunnel.local_addr().port()
        })
    };
    if let Some(existing_port) = existing_port {
        let _ = tunnel.shutdown().await;
        return Ok(existing_port);
    }
    let mut sessions = state.http_tunnel_sessions.lock().map_err(error_string)?;
    sessions.insert(
        key,
        HttpTunnelSession {
            connection_id: request.connection_id.clone(),
            tunnel,
            last_used: Instant::now(),
            active_requests: 1,
        },
    );
    Ok(local_port)
}

fn try_reuse_tunnel(state: &AppState, key: &str) -> Result<Option<u16>, String> {
    let mut sessions = state.http_tunnel_sessions.lock().map_err(error_string)?;
    Ok(sessions.get_mut(key).map(|session| {
        session.active_requests += 1;
        session.last_used = Instant::now();
        session.tunnel.local_addr().port()
    }))
}

fn release_tunnel(state: &AppState, request: &HttpTunnelRequest) {
    if let Ok(mut sessions) = state.http_tunnel_sessions.lock() {
        if let Some(session) = sessions.get_mut(&session_key(request)) {
            session.active_requests = session.active_requests.saturating_sub(1);
            session.last_used = Instant::now();
        }
    }
}

fn schedule_idle_cleanup(state: AppState, key: String) {
    tokio::spawn(async move {
        tokio::time::sleep(TUNNEL_IDLE_TTL).await;
        let session = match state.http_tunnel_sessions.lock() {
            Ok(mut sessions) => {
                let should_remove = sessions.get(&key).is_some_and(|session| {
                    session.active_requests == 0 && session.last_used.elapsed() >= TUNNEL_IDLE_TTL
                });
                if should_remove {
                    sessions.remove(&key)
                } else {
                    None
                }
            }
            Err(_) => None,
        };
        if let Some(session) = session {
            let _ = session.shutdown().await;
        }
    });
}

fn parse_request(args: Vec<Value>) -> Result<HttpTunnelRequest, String> {
    if args.len() == 1 && args[0].is_object() {
        return serde_json::from_value(args[0].clone())
            .map_err(|error| format!("HTTP 隧道请求参数无效：{error}"));
    }

    let connection_id = string_arg(&args, 0)?;
    let target_host = string_arg(&args, 1)?;
    let target_port = args
        .get(2)
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .ok_or_else(|| "HTTP 目标端口必须在 1-65535 范围内。".to_string())?;
    let path = string_arg(&args, 3)?;
    let auth = args
        .get(4)
        .cloned()
        .filter(|value| !value.is_null())
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| format!("HTTP Basic Auth 参数无效：{error}"))?;
    let body = args.get(5).cloned().filter(|value| !value.is_null());
    let ignore_ssl = args.get(6).and_then(Value::as_bool).unwrap_or(false);
    let secure = args.get(7).and_then(Value::as_bool);

    Ok(HttpTunnelRequest {
        connection_id,
        target_host,
        target_port,
        path,
        auth,
        body,
        ignore_ssl,
        secure,
    })
}

fn validate_target(host: &str, port: u16) -> Result<(), String> {
    if host.trim().is_empty() {
        return Err("HTTP 目标主机不能为空。".to_string());
    }
    if port == 0 {
        return Err("HTTP 目标端口必须在 1-65535 范围内。".to_string());
    }
    Ok(())
}

fn session_key(request: &HttpTunnelRequest) -> String {
    format!(
        "{}:{}:{}",
        request.connection_id,
        request.target_host.trim().to_ascii_lowercase(),
        request.target_port
    )
}

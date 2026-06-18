use base64::Engine;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    time::Duration,
};
use tauri::Emitter;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    time,
};

use crate::{error_string, random_id, AppState, SshProfile};

pub(crate) struct AskpassBroker {
    pub(crate) address: String,
    pub(crate) token: String,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for AskpassBroker {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

pub(crate) fn run_askpass_helper_from_env() -> Option<i32> {
    if std::env::var("SHELLDESK_ASKPASS_HELPER").ok().as_deref() != Some("1") {
        return None;
    }
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let encoded_password = std::env::var("SHELLDESK_ASKPASS_PASSWORD").unwrap_or_default();
    let result = match decode_askpass_password(&encoded_password) {
        Ok(password) if !password.is_empty() && is_password_keyboard_prompt(&prompt) => {
            println!("{password}");
            Ok(())
        }
        _ => request_askpass_broker_response(&prompt).map(|response| {
            println!("{response}");
        }),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        return Some(1);
    }
    Some(0)
}

fn decode_askpass_password(encoded: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "ShellDesk askpass 密码格式无效。".to_string())?;
    String::from_utf8(bytes).map_err(|_| "ShellDesk askpass 密码格式无效。".to_string())
}

fn request_askpass_broker_response(prompt: &str) -> Result<String, String> {
    let address = std::env::var("SHELLDESK_ASKPASS_BROKER")
        .map_err(|_| "ShellDesk askpass 没有可用交互通道。".to_string())?;
    let token = std::env::var("SHELLDESK_ASKPASS_TOKEN")
        .map_err(|_| "ShellDesk askpass 交互令牌缺失。".to_string())?;
    let mut stream = std::net::TcpStream::connect(address).map_err(error_string)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(180)))
        .map_err(error_string)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(error_string)?;
    let request = json!({
        "token": token,
        "prompt": prompt,
    });
    stream
        .write_all(format!("{request}\n").as_bytes())
        .map_err(error_string)?;
    stream.flush().map_err(error_string)?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(error_string)?;
    let response: Value = serde_json::from_str(line.trim())
        .map_err(|_| "ShellDesk askpass 交互响应格式无效。".to_string())?;
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("ShellDesk askpass 交互已取消。")
            .to_string());
    }
    Ok(response
        .get("response")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
}

fn is_password_keyboard_prompt(prompt: &str) -> bool {
    let prompt = prompt.trim().to_lowercase();
    if prompt.is_empty() {
        return false;
    }
    let password_like = ["password", "passphrase", "passcode", "密码", "口令"]
        .iter()
        .any(|needle| prompt.contains(needle));
    if !password_like {
        return false;
    }
    ![
        "one-time",
        "one time",
        "otp",
        "totp",
        "token",
        "verification",
        "verify",
        "code",
        "验证码",
        "动态",
        "令牌",
        "一次",
    ]
    .iter()
    .any(|needle| prompt.contains(needle))
}

pub(crate) fn remember_ui_window(state: &AppState, window: &tauri::Window) {
    if let Ok(mut current) = state.ui_window.lock() {
        *current = Some(window.clone());
    }
}

pub(crate) fn current_ui_window(state: &AppState) -> Option<tauri::Window> {
    state
        .ui_window
        .lock()
        .ok()
        .and_then(|current| current.as_ref().cloned())
}

pub(crate) async fn start_askpass_broker(
    state: &AppState,
    window: tauri::Window,
    profile: SshProfile,
) -> Result<AskpassBroker, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(error_string)?;
    let address = listener.local_addr().map_err(error_string)?.to_string();
    let token = random_id("askpass");
    let broker_token = token.clone();
    let broker_state = state.clone();
    let (shutdown_sender, mut shutdown_receiver) = oneshot::channel();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_receiver => break,
                accepted = listener.accept() => {
                    let Ok((stream, _peer)) = accepted else {
                        continue;
                    };
                    let state = broker_state.clone();
                    let window = window.clone();
                    let profile = profile.clone();
                    let token = broker_token.clone();
                    tokio::spawn(async move {
                        let _ = handle_askpass_broker_client(state, window, profile, token, stream).await;
                    });
                }
            }
        }
    });

    Ok(AskpassBroker {
        address,
        token,
        shutdown: Some(shutdown_sender),
    })
}

async fn handle_askpass_broker_client(
    state: AppState,
    window: tauri::Window,
    profile: SshProfile,
    token: String,
    stream: TcpStream,
) -> Result<(), String> {
    let mut reader = TokioBufReader::new(stream);
    let mut line = String::new();
    let read = time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .map_err(|_| "ShellDesk askpass 请求超时。".to_string())?
        .map_err(error_string)?;
    let mut stream = reader.into_inner();
    if read == 0 {
        return Ok(());
    }
    let request: Value = serde_json::from_str(line.trim())
        .map_err(|_| "ShellDesk askpass 请求格式无效。".to_string())?;
    let response = if request.get("token").and_then(Value::as_str) != Some(token.as_str()) {
        json!({ "ok": false, "error": "ShellDesk askpass 令牌无效。" })
    } else {
        let prompt = request
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match request_keyboard_interactive_decision(&state, &window, &profile, prompt).await {
            Ok(answer) => json!({ "ok": true, "response": answer }),
            Err(error) => json!({ "ok": false, "error": error }),
        }
    };
    stream
        .write_all(format!("{response}\n").as_bytes())
        .await
        .map_err(error_string)?;
    stream.flush().await.map_err(error_string)
}

async fn request_keyboard_interactive_decision(
    state: &AppState,
    window: &tauri::Window,
    profile: &SshProfile,
    prompt: String,
) -> Result<String, String> {
    let request_id = random_id("keyboard");
    let (sender, receiver) = oneshot::channel();
    state
        .keyboard_interactive_responses
        .lock()
        .map_err(error_string)?
        .insert(request_id.clone(), sender);
    let payload = json!({
        "requestId": request_id,
        "hostname": profile.address,
        "port": profile.port,
        "username": profile.username,
        "name": "ShellDesk",
        "instructions": "",
        "prompts": [{
            "prompt": prompt,
            "echo": false
        }]
    });
    if let Err(error) = window.emit("connection:keyboard-interactive", payload) {
        let _ = state
            .keyboard_interactive_responses
            .lock()
            .map_err(error_string)?
            .remove(&request_id);
        return Err(error_string(error));
    }

    let response = match time::timeout(Duration::from_secs(180), receiver).await {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => return Err("SSH 交互认证请求已关闭。".to_string()),
        Err(_) => {
            let _ = state
                .keyboard_interactive_responses
                .lock()
                .map_err(error_string)?
                .remove(&request_id);
            return Err("SSH 交互认证超时。".to_string());
        }
    };
    if response
        .get("cancel")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("已取消 SSH 交互认证。".to_string());
    }
    Ok(response
        .get("responses")
        .and_then(Value::as_array)
        .and_then(|responses| responses.first())
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
}

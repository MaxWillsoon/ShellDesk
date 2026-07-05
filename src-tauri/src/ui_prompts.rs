use serde_json::{json, Value};
use std::time::Duration;
use tokio::{sync::oneshot, time};

use crate::{error_string, random_id, AppState, SshProfile, UiWindowRef};

pub(crate) fn remember_ui_window(state: &AppState, window: &tauri::Window) {
    if let Ok(mut current) = state.ui_window.lock() {
        *current = Some(UiWindowRef::from_window(window));
    }
}

pub(crate) fn current_ui_window(state: &AppState) -> Option<tauri::Window> {
    state
        .ui_window
        .lock()
        .ok()
        .and_then(|current| current.as_ref().map(|current| current.window.clone()))
}

pub(crate) async fn request_keyboard_interactive_decision(
    state: AppState,
    window: UiWindowRef,
    profile: SshProfile,
    name: String,
    instructions: String,
    prompts: Vec<(String, bool)>,
) -> Result<Vec<String>, String> {
    let request_id = random_id("keyboard");
    let (sender, receiver) = oneshot::channel();
    state
        .keyboard_interactive_responses
        .lock()
        .map_err(error_string)?
        .insert(request_id.clone(), sender);
    let payload_prompts = prompts
        .into_iter()
        .map(|(prompt, echo)| json!({ "prompt": prompt, "echo": echo }))
        .collect::<Vec<_>>();
    let payload = json!({
        "requestId": request_id,
        "hostname": profile.address,
        "port": profile.port,
        "username": profile.username,
        "name": if name.trim().is_empty() { "ShellDesk" } else { name.as_str() },
        "instructions": instructions,
        "prompts": payload_prompts
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
        .map(|responses| {
            responses
                .iter()
                .map(|response| response.as_str().unwrap_or("").to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default())
}

use crate::{connection, error_string, node_platform, string_arg, AppState};
use serde_json::{json, Value};
use tauri::{
    utils::config::Color, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

const PACKAGE_JSON: &str = include_str!("../../package.json");
const DEFAULT_REPOSITORY: &str = "https://github.com/liubaicai/ShellDesk";

#[derive(serde::Serialize)]
pub(crate) struct AppInfo {
    name: String,
    #[serde(rename = "productName")]
    product_name: String,
    version: String,
    description: String,
    homepage: String,
    author: String,
    platform: String,
    arch: String,
    #[serde(rename = "isPackaged")]
    is_packaged: bool,
}

pub(crate) fn get_info(app: &tauri::AppHandle) -> AppInfo {
    build_app_info(
        &app.package_info().version.to_string(),
        !cfg!(debug_assertions),
    )
}

fn build_app_info(version: &str, is_packaged: bool) -> AppInfo {
    let metadata = package_metadata();
    AppInfo {
        name: metadata
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("shelldesk")
            .to_string(),
        product_name: metadata
            .get("productName")
            .and_then(Value::as_str)
            .or_else(|| metadata.get("name").and_then(Value::as_str))
            .unwrap_or("ShellDesk")
            .to_string(),
        version: version.to_string(),
        description: metadata
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        homepage: metadata
            .get("homepage")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_REPOSITORY)
            .to_string(),
        author: metadata
            .get("author")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        platform: node_platform(),
        arch: std::env::consts::ARCH.to_string(),
        is_packaged,
    }
}

fn package_metadata() -> Value {
    serde_json::from_str(PACKAGE_JSON).unwrap_or_else(|_| json!({}))
}

pub(crate) fn open_external(args: Vec<Value>) -> Result<Value, String> {
    let url = string_arg(&args, 0)?;
    if url.len() > 2048 || !is_safe_external_url(&url) {
        return Err("外部链接不受支持。".to_string());
    }
    open::that(url).map_err(error_string)?;
    Ok(json!(true))
}

pub(crate) fn open_connection_window(
    app: &tauri::AppHandle,
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let connection = connection::get_connection(state, &connection_id)?;
    let host = &connection.host;
    let is_local_connection = connection.kind == crate::state::ConnectionKind::Local;
    let username = host.get("username").and_then(Value::as_str).unwrap_or("");
    let address = host.get("address").and_then(Value::as_str).unwrap_or("SSH");
    let port = host.get("port").and_then(Value::as_u64).unwrap_or(22);
    let connection_title = if is_local_connection {
        "本地模式".to_string()
    } else if username.trim().is_empty() {
        format!("{address}:{port}")
    } else {
        format!("{username}@{address}:{port}")
    };
    let title = if is_local_connection || connection.proxy_port == 0 {
        format!("ShellDesk - {connection_title}")
    } else {
        format!(
            "ShellDesk - {connection_title} - SOCKS :{}",
            connection.proxy_port
        )
    };
    let label = format!("connection-{}", sanitize_window_label(&connection_id));
    let url = format!("index.html?connectionId={}", url_component(&connection_id));

    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(error_string)?;
        window.set_focus().map_err(error_string)?;
        return Ok(json!({ "ok": true, "label": label }));
    }

    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(1240.0, 800.0)
        .min_inner_size(960.0, 640.0)
        .background_color(Color(14, 19, 28, 255))
        .decorations(cfg!(target_os = "macos"))
        .resizable(true)
        .visible(true)
        .build()
        .map_err(error_string)?;

    let state = state.clone();
    let connection_id_for_close = connection_id.clone();
    let window_for_close = window.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let _ = connection::close_connection_by_id(&state, &connection_id_for_close);
            let _ = window_for_close.emit(
                "connection:closed",
                json!({ "connectionId": connection_id_for_close, "reason": "连接窗口已关闭。" }),
            );
        }
    });
    let _ = window.emit(
        "window:maximize-state-changed",
        json!({ "maximized": false }),
    );

    Ok(json!({ "ok": true, "label": label }))
}

fn sanitize_window_label(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn url_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn is_safe_external_url(raw_url: &str) -> bool {
    reqwest::Url::parse(raw_url)
        .map(|url| matches!(url.scheme(), "https" | "http" | "mailto"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_external_url_allows_legacy_protocols() {
        assert!(is_safe_external_url("https://example.com/path?q=1"));
        assert!(is_safe_external_url("http://127.0.0.1:8080"));
        assert!(is_safe_external_url("mailto:admin@example.com"));
    }

    #[test]
    fn safe_external_url_rejects_local_paths_and_unsafe_protocols() {
        assert!(!is_safe_external_url(
            "file:///C:/Windows/System32/calc.exe"
        ));
        assert!(!is_safe_external_url("C:\\Windows\\System32\\calc.exe"));
        assert!(!is_safe_external_url("javascript:alert(1)"));
        assert!(!is_safe_external_url("shell:AppsFolder"));
    }

    #[test]
    fn app_info_uses_package_metadata() {
        let info = build_app_info("9.8.7", true);

        assert_eq!(info.name, "shelldesk");
        assert_eq!(info.product_name, "ShellDesk");
        assert_eq!(info.version, "9.8.7");
        assert_eq!(info.description, "Tauri shell for a GUI SSH client.");
        assert_eq!(info.homepage, DEFAULT_REPOSITORY);
        assert_eq!(info.author, "liubaicai <liushuai.baicai@hotmail.com>");
        assert!(info.is_packaged);
    }
}

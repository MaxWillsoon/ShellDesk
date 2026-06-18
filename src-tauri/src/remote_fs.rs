use crate::command_runner::run_shell;
use crate::local_fs::{
    create_local_directory, create_local_file, delete_local_path, list_local_directory,
    read_local_file, rename_local_path, set_local_path_permissions, stat_local_path,
    write_local_file,
};
use crate::{
    error_string, get_connection, prevent_process_window, prevent_tokio_process_window, random_id,
    read_string_field, run_connection_command_with_options,
    run_ssh_command_for_profile_interactive, shell_quote, string_arg, ActiveConnection,
    ActiveTransfer, AppState, ConnectionKind, SshProfile,
};
use base64::Engine;
use chrono::Utc;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command as StdCommand,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::Emitter;
use tokio::process::Command;

const ALL_FILES_FILTER_NAME: &str = "所有文件";
const UPLOAD_FILES_TITLE: &str = "选择要上传的文件";
const UPLOAD_FOLDERS_TITLE: &str = "选择要上传的文件夹";
const DOWNLOAD_DIRECTORY_TITLE: &str = "选择下载保存目录";
const TRANSFER_COPY_CHUNK_BYTES: usize = 256 * 1024;

pub(crate) async fn list_connection_directory(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return list_local_directory(vec![
            json!(connection_id),
            json!(remote_path),
            args.get(2).cloned().unwrap_or(Value::Null),
        ]);
    }
    let command = remote_list_directory_command(&connection, &remote_path);
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(command),
            json!(""),
            args.get(2).cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    if remote_host_is_windows(&connection) {
        command_json(output, "列出远程目录失败。")
    } else {
        parse_unix_directory_listing(output, remote_path)
    }
}

pub(crate) async fn stat_connection_path(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return stat_local_path(vec![
            json!(connection_id),
            json!(remote_path),
            args.get(2).cloned().unwrap_or(Value::Null),
        ]);
    }
    let command = remote_stat_path_command(&connection, &remote_path);
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(command),
            json!(""),
            args.get(2).cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    if remote_host_is_windows(&connection) {
        command_json(output, "读取远程路径属性失败。")
    } else {
        parse_unix_path_stat(output)
    }
}

pub(crate) async fn read_connection_file(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return read_local_file(vec![
            json!(connection_id),
            json!(remote_path),
            args.get(2).cloned().unwrap_or(Value::Null),
        ]);
    }
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(remote_read_file_command(&connection, &remote_path)),
            json!(""),
            args.get(2).cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("读取远程文件失败。")
            .to_string());
    }
    Ok(json!(output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")))
}

pub(crate) async fn write_connection_file(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let content = args
        .get(2)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return write_local_file(vec![
            json!(connection_id),
            json!(remote_path),
            json!(content),
            args.get(3).cloned().unwrap_or(Value::Null),
        ]);
    }
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(remote_write_file_command(&connection, &remote_path)),
            json!(content),
            args.get(3).cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) == 0 {
        Ok(json!(true))
    } else {
        Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("写入远程文件失败。")
            .to_string())
    }
}

pub(crate) async fn create_connection_directory(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return create_local_directory(vec![
            json!(connection_id),
            json!(remote_path),
            args.get(2).cloned().unwrap_or(Value::Null),
        ]);
    }
    let command = remote_create_directory_command(&connection, &remote_path);
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(command),
            json!(""),
            args.get(2).cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    command_bool(output, "创建远程目录失败。")
}

pub(crate) async fn create_connection_file(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return create_local_file(vec![
            json!(connection_id),
            json!(remote_path),
            args.get(2).cloned().unwrap_or(Value::Null),
        ]);
    }
    let command = remote_create_file_command(&connection, &remote_path);
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(command),
            json!(""),
            args.get(2).cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    command_bool(output, "创建远程文件失败。")
}

pub(crate) async fn delete_connection_path(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let entry_type = args
        .get(2)
        .and_then(Value::as_str)
        .unwrap_or("file")
        .to_string();
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return delete_local_path(vec![
            json!(connection_id),
            json!(remote_path),
            json!(entry_type),
            args.get(3).cloned().unwrap_or(Value::Null),
        ]);
    }
    let command = remote_delete_path_command(&connection, &remote_path, &entry_type);
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(command),
            json!(""),
            args.get(3).cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    command_bool(output, "删除远程路径失败。")
}

pub(crate) async fn rename_connection_path(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let old_path = string_arg(&args, 1)?;
    let new_path = string_arg(&args, 2)?;
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return rename_local_path(vec![
            json!(connection_id),
            json!(old_path),
            json!(new_path),
            args.get(3).cloned().unwrap_or(Value::Null),
        ]);
    }
    let command = remote_rename_path_command(&connection, &old_path, &new_path);
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(command),
            json!(""),
            args.get(3).cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    command_bool(output, "重命名远程路径失败。")
}

pub(crate) async fn check_connection_sftp(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return Ok(json!({ "available": true }));
    }
    let profile = connection
        .ssh
        .ok_or_else(|| "SSH profile is unavailable.".to_string())?;
    let output = run_ssh_command_for_profile_interactive(
        state,
        profile,
        remote_sftp_probe_command(),
        String::new(),
    )
    .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) == 0 {
        return Ok(json!({ "available": true }));
    }
    Ok(json!({
        "available": false,
        "error": output
            .get("stderr")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("远程系统未找到可执行的 sftp-server。")
    }))
}

pub(crate) fn remote_sftp_probe_command() -> String {
    [
        "for candidate in /usr/lib/openssh/sftp-server /usr/libexec/openssh/sftp-server /usr/lib/ssh/sftp-server /usr/libexec/sftp-server /usr/local/libexec/sftp-server /usr/local/lib/sftp-server; do",
        "  if [ -x \"$candidate\" ]; then exit 0; fi",
        "done",
        "command -v sftp-server >/dev/null 2>&1",
    ]
    .join("\n")
}

pub(crate) fn select_upload_items(folders: bool) -> Result<Value, String> {
    let paths = if folders {
        rfd::FileDialog::new()
            .set_title(UPLOAD_FOLDERS_TITLE)
            .pick_folders()
    } else {
        rfd::FileDialog::new()
            .set_title(UPLOAD_FILES_TITLE)
            .add_filter(ALL_FILES_FILTER_NAME, &["*"])
            .pick_files()
    };
    let Some(paths) = paths else {
        return Ok(json!({ "canceled": true, "items": [] }));
    };
    let mut items = Vec::new();
    for path in paths {
        let metadata = fs::metadata(&path).map_err(error_string)?;
        items.push(json!({
            "path": path.to_string_lossy(),
            "name": path.file_name().map(|value| value.to_string_lossy().to_string()).unwrap_or_else(|| "upload".to_string()),
            "type": if metadata.is_dir() { "directory" } else { "file" },
            "size": if metadata.is_file() { metadata.len() } else { 0 },
            "modifiedAt": metadata.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).and_then(|duration| chrono::DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, 0)).unwrap_or_else(Utc::now).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        }));
    }
    Ok(json!({ "canceled": false, "items": items }))
}

struct TransferReporter {
    window: tauri::Window,
    cancellations: Arc<Mutex<HashSet<String>>>,
    active_transfers: Arc<Mutex<std::collections::HashMap<String, ActiveTransfer>>>,
    state: Arc<Mutex<TransferReporterState>>,
}

struct TransferReporterState {
    connection_id: String,
    queue_id: String,
    client_id: Option<String>,
    transfer_type: String,
    file_name: String,
    transferred: u64,
    total: u64,
    current_file_transferred: u64,
    current_file_total: u64,
    completed_files: u64,
    total_files: u64,
    completed_items: u64,
    total_items: u64,
    started: bool,
    ended: bool,
    registered: bool,
}

impl TransferReporter {
    fn new(
        app_state: &AppState,
        window: &tauri::Window,
        connection_id: &str,
        transfer_type: &str,
        options: Option<&Value>,
        file_name: String,
    ) -> Self {
        let client_id = options
            .and_then(|value| {
                value
                    .get("transferClientId")
                    .or_else(|| value.get("clientId"))
            })
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let queue_id = options
            .and_then(|value| value.get("queueId"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| random_id("transfer"));

        Self {
            window: window.clone(),
            cancellations: app_state.transfer_cancellations.clone(),
            active_transfers: app_state.active_transfers.clone(),
            state: Arc::new(Mutex::new(TransferReporterState {
                connection_id: connection_id.to_string(),
                queue_id,
                client_id,
                transfer_type: transfer_type.to_string(),
                file_name,
                transferred: 0,
                total: 0,
                current_file_transferred: 0,
                current_file_total: 0,
                completed_files: 0,
                total_files: 0,
                completed_items: 0,
                total_items: 0,
                started: false,
                ended: false,
                registered: false,
            })),
        }
    }

    fn set_totals(&self, total: u64, total_files: u64, total_items: u64) {
        self.register_active();
        if let Ok(mut state) = self.state.lock() {
            state.total = total;
            state.total_files = total_files;
            state.total_items = total_items;
            state.started = true;
        }
        self.emit_progress();
    }

    fn start_file(&self, file_name: &str, current_file_total: u64) {
        self.register_active();
        if let Ok(mut state) = self.state.lock() {
            state.file_name = file_name.to_string();
            state.current_file_transferred = 0;
            state.current_file_total = current_file_total;
            if state.total == 0 && current_file_total > 0 {
                state.total = current_file_total;
            }
            state.started = true;
        }
        self.emit_progress();
    }

    fn add_bytes(&self, bytes: u64) {
        self.register_active();
        if let Ok(mut state) = self.state.lock() {
            state.transferred = state.transferred.saturating_add(bytes);
            state.current_file_transferred = state.current_file_transferred.saturating_add(bytes);
            if state.total < state.transferred {
                state.total = state.transferred;
            }
            if state.current_file_total < state.current_file_transferred {
                state.current_file_total = state.current_file_transferred;
            }
            state.started = true;
        }
        self.emit_progress();
    }

    fn complete_file(&self) {
        self.register_active();
        if let Ok(mut state) = self.state.lock() {
            state.completed_files = state.completed_files.saturating_add(1);
            state.completed_items = state.completed_items.saturating_add(1);
            if state.current_file_total > 0 {
                state.current_file_transferred = state.current_file_total;
            }
            state.started = true;
        }
        self.emit_progress();
    }

    fn check_canceled(&self) -> Result<(), String> {
        self.register_active();
        let (queue_id, client_id) = self.ids();
        let canceled = self
            .cancellations
            .lock()
            .map_err(error_string)?
            .iter()
            .any(|id| {
                id == &queue_id || client_id.as_ref().is_some_and(|client_id| id == client_id)
            });
        if canceled {
            let message = "传输已取消。".to_string();
            self.finish(false, Some(&message));
            return Err(message);
        }
        Ok(())
    }

    fn finish(&self, success: bool, error: Option<&str>) {
        let mut payload = None;
        let mut ids = None;
        if let Ok(mut state) = self.state.lock() {
            if state.ended {
                return;
            }
            state.ended = true;
            ids = Some((state.queue_id.clone(), state.client_id.clone()));
            payload = Some(state.payload(success, error));
        }
        if let Some((queue_id, client_id)) = ids {
            self.unregister(&queue_id, client_id.as_deref());
        }
        if let Some(payload) = payload {
            let _ = self.window.emit("transfer:end", payload);
        }
    }

    fn register_active(&self) {
        let mut registration = None;
        if let Ok(mut state) = self.state.lock() {
            if state.ended || state.registered {
                return;
            }
            state.registered = true;
            registration = Some((
                state.queue_id.clone(),
                ActiveTransfer {
                    connection_id: state.connection_id.clone(),
                    client_id: state.client_id.clone(),
                },
            ));
        }
        if let Some((queue_id, transfer)) = registration {
            if let Ok(mut active_transfers) = self.active_transfers.lock() {
                active_transfers.insert(queue_id, transfer);
            }
        }
    }

    fn unregister(&self, queue_id: &str, client_id: Option<&str>) {
        if let Ok(mut cancellations) = self.cancellations.lock() {
            cancellations.remove(queue_id);
            if let Some(client_id) = client_id {
                cancellations.remove(client_id);
            }
        }
        if let Ok(mut active_transfers) = self.active_transfers.lock() {
            active_transfers.remove(queue_id);
        }
    }

    fn emit_progress(&self) {
        if let Ok(state) = self.state.lock() {
            if state.ended {
                return;
            }
            let _ = self
                .window
                .emit("transfer:progress", state.payload(false, None));
        }
    }

    fn ids(&self) -> (String, Option<String>) {
        self.state
            .lock()
            .map(|state| (state.queue_id.clone(), state.client_id.clone()))
            .unwrap_or_else(|_| (String::new(), None))
    }
}

impl Drop for TransferReporter {
    fn drop(&mut self) {
        let mut ids = None;
        let should_finish = self
            .state
            .lock()
            .map(|state| {
                ids = Some((state.queue_id.clone(), state.client_id.clone()));
                state.started && !state.ended
            })
            .unwrap_or(false);
        if should_finish {
            self.finish(false, Some("传输中断。"));
        } else if let Some((queue_id, client_id)) = ids {
            self.unregister(&queue_id, client_id.as_deref());
        }
    }
}

impl TransferReporterState {
    fn payload(&self, success: bool, error: Option<&str>) -> Value {
        let mut payload = json!({
            "connectionId": self.connection_id,
            "queueId": self.queue_id,
            "type": self.transfer_type,
            "fileName": self.file_name,
            "transferred": self.transferred,
            "total": self.total,
            "currentFileTransferred": self.current_file_transferred,
            "currentFileTotal": self.current_file_total,
            "completedFiles": self.completed_files,
            "totalFiles": self.total_files,
            "completedItems": self.completed_items,
            "totalItems": self.total_items,
            "success": success,
        });
        if let Some(client_id) = &self.client_id {
            payload["clientId"] = json!(client_id);
        }
        if let Some(error) = error {
            payload["error"] = json!(error);
        }
        payload
    }
}

pub(crate) fn cancel_transfer(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let requested_id = args.get(1).and_then(Value::as_str).unwrap_or("").trim();
    if requested_id.is_empty() {
        return cancel_transfers_for_connection(state, &connection_id)
            .map(|canceled| json!(canceled));
    }
    let mut cancel_ids = Vec::new();
    {
        let active_transfers = state.active_transfers.lock().map_err(error_string)?;
        if let Some((queue_id, transfer)) = active_transfers.iter().find(|(queue_id, transfer)| {
            transfer.connection_id == connection_id
                && (*queue_id == requested_id
                    || transfer
                        .client_id
                        .as_deref()
                        .is_some_and(|client_id| client_id == requested_id))
        }) {
            cancel_ids.push(queue_id.clone());
            if let Some(client_id) = &transfer.client_id {
                cancel_ids.push(client_id.clone());
            }
        }
    }
    if cancel_ids.is_empty() {
        return Ok(json!(false));
    }
    let mut cancellations = state.transfer_cancellations.lock().map_err(error_string)?;
    for id in cancel_ids {
        cancellations.insert(id);
    }
    Ok(json!(true))
}

pub(crate) fn cancel_transfers_for_connection(
    state: &AppState,
    connection_id: &str,
) -> Result<bool, String> {
    let mut cancel_ids = Vec::new();
    {
        let active_transfers = state.active_transfers.lock().map_err(error_string)?;
        for (queue_id, transfer) in active_transfers.iter() {
            if transfer.connection_id == connection_id {
                cancel_ids.push(queue_id.clone());
                if let Some(client_id) = &transfer.client_id {
                    cancel_ids.push(client_id.clone());
                }
            }
        }
    }
    if cancel_ids.is_empty() {
        return Ok(false);
    }
    let mut cancellations = state.transfer_cancellations.lock().map_err(error_string)?;
    for id in cancel_ids {
        cancellations.insert(id);
    }
    Ok(true)
}

fn default_transfer_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "transfer".to_string())
}

fn remote_file_name(path: &str, fallback: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty())
        .next_back()
        .unwrap_or(fallback)
        .to_string()
}

fn sanitize_local_file_name(file_name: &str, fallback: &str) -> String {
    let safe_name = file_name
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>()
        .trim_end_matches(['.', ' '])
        .trim()
        .to_string();
    if safe_name.is_empty() || is_windows_reserved_local_file_name(&safe_name) {
        fallback.to_string()
    } else {
        safe_name
    }
}

fn upload_remote_name(item: &Value, local_path: &Path) -> String {
    let fallback_name = remote_file_name(&local_path.to_string_lossy(), "upload");
    let raw_name = item
        .get("remoteName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&fallback_name);
    sanitize_local_file_name(raw_name, "upload")
}

fn is_windows_reserved_local_file_name(file_name: &str) -> bool {
    let stem = file_name
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        stem.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

pub(crate) async fn upload_selected_paths(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
    folders: bool,
    multiple: bool,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_dir = string_arg(&args, 1)?;
    let paths = if folders {
        rfd::FileDialog::new()
            .set_title(UPLOAD_FOLDERS_TITLE)
            .pick_folders()
    } else if multiple {
        rfd::FileDialog::new()
            .set_title(UPLOAD_FILES_TITLE)
            .add_filter(ALL_FILES_FILTER_NAME, &["*"])
            .pick_files()
    } else {
        rfd::FileDialog::new()
            .add_filter(ALL_FILES_FILTER_NAME, &["*"])
            .pick_file()
            .map(|path| vec![path])
    };
    let Some(paths) = paths else {
        return Ok(json!({ "canceled": true }));
    };
    let items = paths
        .into_iter()
        .filter_map(|path| {
            let metadata = fs::metadata(&path).ok()?;
            Some(json!({
                "path": path.to_string_lossy(),
                "name": path.file_name().map(|value| value.to_string_lossy().to_string()).unwrap_or_else(|| "upload".to_string()),
                "type": if metadata.is_dir() { "directory" } else { "file" },
                "size": if metadata.is_file() { metadata.len() } else { 0 }
            }))
        })
        .collect::<Vec<_>>();
    upload_connection_paths(
        state,
        window,
        vec![
            json!(connection_id),
            json!(remote_dir),
            json!(items),
            args.get(2).cloned().unwrap_or(Value::Null),
        ],
    )
    .await
}

pub(crate) async fn download_connection_file(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let transfer = TransferReporter::new(
        state,
        window,
        &connection_id,
        "download",
        args.get(2),
        default_transfer_name(&remote_path),
    );
    let default_name =
        sanitize_local_file_name(&remote_file_name(&remote_path, "download"), "download");
    let Some(local_path) = rfd::FileDialog::new()
        .set_file_name(&default_name)
        .add_filter(ALL_FILES_FILTER_NAME, &["*"])
        .save_file()
    else {
        return Ok(json!({ "canceled": true }));
    };
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        let size = fs::metadata(&remote_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        transfer.set_totals(size, 1, 1);
        transfer.start_file(&default_name, size);
        let _copied =
            copy_local_file_with_transfer(&transfer, Path::new(&remote_path), &local_path)?;
        transfer.complete_file();
    } else {
        let profile = connection
            .ssh
            .ok_or_else(|| "SSH profile is unavailable.".to_string())?;
        transfer.set_totals(0, 1, 1);
        transfer.start_file(&default_name, 0);
        transfer.check_canceled()?;
        let bytes = read_remote_file_bytes_with_options(
            state,
            &connection_id,
            profile,
            &remote_path,
            args.get(2),
        )
        .await?;
        transfer.add_bytes(bytes.len() as u64);
        fs::write(&local_path, &bytes).map_err(error_string)?;
        transfer.complete_file();
    }
    let size = fs::metadata(&local_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    transfer.finish(true, None);
    Ok(json!({
        "canceled": false,
        "filePath": local_path.to_string_lossy(),
        "size": size
    }))
}

pub(crate) async fn download_connection_paths(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_paths = args
        .get(1)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let transfer = TransferReporter::new(
        state,
        window,
        &connection_id,
        "download",
        args.get(2),
        "download".to_string(),
    );
    let Some(local_dir) = rfd::FileDialog::new()
        .set_title(DOWNLOAD_DIRECTORY_TITLE)
        .pick_folder()
    else {
        return Ok(json!({ "canceled": true }));
    };
    let connection = get_connection(state, &connection_id)?;
    let mut total_size = 0_u64;
    let mut file_count = 0_u64;
    let mut item_count = 0_u64;
    transfer.set_totals(0, 0, remote_paths.len() as u64);

    for value in remote_paths {
        transfer.check_canceled()?;
        let Some(remote_path) = value.as_str() else {
            continue;
        };
        let file_name = remote_file_name(remote_path, "download");
        let local_path = local_dir.join(sanitize_local_file_name(&file_name, "download"));
        if connection.kind == ConnectionKind::Local {
            let metadata = fs::metadata(remote_path).map_err(error_string)?;
            if metadata.is_file() {
                transfer.start_file(&file_name, metadata.len());
                let copied =
                    copy_local_file_with_transfer(&transfer, Path::new(remote_path), &local_path)?;
                transfer.complete_file();
                total_size += copied;
                file_count += 1;
                item_count += 1;
            } else if metadata.is_dir() {
                let (copied, files) =
                    copy_local_path_with_transfer(&transfer, Path::new(remote_path), &local_path)?;
                total_size += copied;
                file_count += files;
                item_count += 1;
            }
        } else {
            let profile = connection
                .ssh
                .clone()
                .ok_or_else(|| "SSH profile is unavailable.".to_string())?;
            let kind = remote_path_kind(state, profile.clone(), remote_path).await?;
            if kind == "directory" {
                let size = remote_path_size(state, profile.clone(), remote_path)
                    .await
                    .unwrap_or(0);
                transfer.start_file(&file_name, size);
                let bytes = download_remote_directory_archive_with_options(
                    state,
                    &connection_id,
                    profile,
                    remote_path,
                    args.get(2),
                )
                .await?;
                extract_tar_gz_archive(&bytes, &local_dir)?;
                transfer.add_bytes(size.max(bytes.len() as u64));
                transfer.complete_file();
                total_size += size.max(bytes.len() as u64);
                file_count += 1;
                item_count += 1;
            } else {
                let size = remote_path_size(state, profile.clone(), remote_path)
                    .await
                    .unwrap_or(0);
                transfer.start_file(&file_name, size);
                let bytes = read_remote_file_bytes_with_options(
                    state,
                    &connection_id,
                    profile,
                    remote_path,
                    args.get(2),
                )
                .await?;
                fs::write(&local_path, &bytes).map_err(error_string)?;
                transfer.add_bytes(bytes.len() as u64);
                transfer.complete_file();
                total_size += bytes.len() as u64;
                file_count += 1;
                item_count += 1;
            }
        }
    }

    transfer.finish(true, None);
    Ok(json!({
        "canceled": false,
        "directoryPath": local_dir.to_string_lossy(),
        "size": total_size,
        "fileCount": file_count,
        "itemCount": item_count
    }))
}

pub(crate) async fn upload_connection_paths(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_dir = string_arg(&args, 1)?;
    let items = args
        .get(2)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let item_count = items.len() as u64;
    let transfer = TransferReporter::new(
        state,
        window,
        &connection_id,
        "upload",
        args.get(3),
        "upload".to_string(),
    );
    let connection = get_connection(state, &connection_id)?;
    let mut uploaded_paths = Vec::new();
    let mut total_size = 0_u64;
    let mut file_count = 0_u64;
    let planned_total = items
        .iter()
        .filter_map(|item| {
            let path = PathBuf::from(read_string_field(item, "path", ""));
            Some(local_path_file_stats(&path).0)
        })
        .sum::<u64>();
    let planned_files = items
        .iter()
        .map(|item| {
            let path = PathBuf::from(read_string_field(item, "path", ""));
            local_path_file_stats(&path).1
        })
        .sum::<u64>();
    transfer.set_totals(planned_total, planned_files, items.len() as u64);

    if connection.kind == ConnectionKind::Local {
        fs::create_dir_all(&remote_dir).map_err(error_string)?;
        for item in items {
            transfer.check_canceled()?;
            let local_path = read_string_field(&item, "path", "");
            if local_path.is_empty() {
                continue;
            }
            let local_path_buf = PathBuf::from(&local_path);
            let remote_name = upload_remote_name(&item, &local_path_buf);
            let target = Path::new(&remote_dir).join(remote_name);
            if local_path_buf.is_file() {
                let size = fs::metadata(&local_path_buf).map_err(error_string)?.len();
                transfer.start_file(&default_transfer_name(&local_path), size);
                let copied = copy_local_file_with_transfer(&transfer, &local_path_buf, &target)?;
                transfer.complete_file();
                total_size += copied;
                file_count += 1;
                uploaded_paths.push(json!(target.to_string_lossy()));
            } else if local_path_buf.is_dir() {
                let (copied, files) =
                    copy_local_path_with_transfer(&transfer, &local_path_buf, &target)?;
                total_size += copied;
                file_count += files;
                uploaded_paths.push(json!(target.to_string_lossy()));
            }
        }
    } else {
        let profile = connection
            .ssh
            .clone()
            .ok_or_else(|| "SSH profile is unavailable.".to_string())?;
        for item in items {
            transfer.check_canceled()?;
            let local_path = read_string_field(&item, "path", "");
            if local_path.is_empty() {
                continue;
            }
            let local_path_buf = PathBuf::from(&local_path);
            let remote_name = upload_remote_name(&item, &local_path_buf);
            let remote_path = join_remote_path(&remote_dir, &remote_name);
            if local_path_buf.is_file() {
                let bytes = fs::read(&local_path_buf).map_err(error_string)?;
                transfer.start_file(&remote_name, bytes.len() as u64);
                write_remote_file_bytes_with_options(
                    state,
                    &connection_id,
                    profile.clone(),
                    &remote_path,
                    &bytes,
                    args.get(3),
                )
                .await?;
                transfer.add_bytes(bytes.len() as u64);
                transfer.complete_file();
                let size = fs::metadata(&local_path_buf).map_err(error_string)?.len();
                total_size += size;
                file_count += 1;
                uploaded_paths.push(json!(remote_path));
            } else if local_path_buf.is_dir() {
                let (size, files) = local_path_file_stats(&local_path_buf);
                transfer.start_file(&remote_name, size);
                upload_local_directory_to_remote(
                    state,
                    profile.clone(),
                    &local_path_buf,
                    &remote_dir,
                )
                .await?;
                transfer.add_bytes(size);
                transfer.complete_file();
                total_size += size;
                file_count += files.max(1);
                uploaded_paths.push(json!(remote_path));
            }
        }
    }

    transfer.finish(true, None);
    Ok(json!({
        "canceled": false,
        "remotePath": remote_dir,
        "remotePaths": uploaded_paths,
        "size": total_size,
        "fileCount": file_count,
        "itemCount": item_count
    }))
}

fn local_path_file_stats(path: &Path) -> (u64, u64) {
    let Ok(metadata) = fs::metadata(path) else {
        return (0, 0);
    };
    if metadata.is_file() {
        return (metadata.len(), 1);
    }
    if !metadata.is_dir() {
        return (0, 0);
    }
    let mut total_size = 0_u64;
    let mut file_count = 0_u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let (size, files) = local_path_file_stats(&entry.path());
            total_size = total_size.saturating_add(size);
            file_count = file_count.saturating_add(files);
        }
    }
    (total_size, file_count)
}

fn copy_local_path_with_transfer(
    transfer: &TransferReporter,
    source: &Path,
    target: &Path,
) -> Result<(u64, u64), String> {
    transfer.check_canceled()?;
    let metadata = fs::metadata(source).map_err(error_string)?;
    if metadata.is_file() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(error_string)?;
        }
        let file_name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "transfer".to_string());
        transfer.start_file(&file_name, metadata.len());
        let copied = copy_local_file_with_transfer(transfer, source, target)?;
        transfer.complete_file();
        return Ok((copied, 1));
    }
    if !metadata.is_dir() {
        return Ok((0, 0));
    }
    fs::create_dir_all(target).map_err(error_string)?;
    let mut total_size = 0_u64;
    let mut file_count = 0_u64;
    for entry in fs::read_dir(source).map_err(error_string)? {
        let entry = entry.map_err(error_string)?;
        let child_source = entry.path();
        let child_target = target.join(entry.file_name());
        let (size, files) = copy_local_path_with_transfer(transfer, &child_source, &child_target)?;
        total_size = total_size.saturating_add(size);
        file_count = file_count.saturating_add(files);
    }
    Ok((total_size, file_count))
}

fn copy_local_file_with_transfer(
    transfer: &TransferReporter,
    source: &Path,
    target: &Path,
) -> Result<u64, String> {
    transfer.check_canceled()?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(error_string)?;
    }
    let mut input = fs::File::open(source).map_err(error_string)?;
    let mut output = fs::File::create(target).map_err(error_string)?;
    let mut buffer = vec![0_u8; TRANSFER_COPY_CHUNK_BYTES];
    let mut copied = 0_u64;
    loop {
        transfer.check_canceled()?;
        let read = input.read(&mut buffer).map_err(error_string)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(error_string)?;
        copied = copied.saturating_add(read as u64);
        transfer.add_bytes(read as u64);
    }
    output.flush().map_err(error_string)?;
    Ok(copied)
}

async fn remote_path_kind(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
) -> Result<String, String> {
    let command = format!(
        "if [ -d {path} ]; then echo directory; elif [ -f {path} ]; then echo file; elif [ -L {path} ]; then echo symlink; else echo missing; fi",
        path = shell_quote(remote_path)
    );
    let output =
        run_ssh_command_for_profile_interactive(state, profile, command, String::new()).await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("检查远程路径失败。")
            .to_string());
    }
    Ok(output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string())
}

async fn remote_path_size(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
) -> Result<u64, String> {
    let command = format!(
        "if [ -d {path} ]; then du -sb {path} 2>/dev/null | awk '{{print $1}}'; else stat -c %s {path} 2>/dev/null || wc -c < {path}; fi",
        path = shell_quote(remote_path)
    );
    let output =
        run_ssh_command_for_profile_interactive(state, profile, command, String::new()).await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Ok(0);
    }
    Ok(output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("0")
        .trim()
        .parse::<u64>()
        .unwrap_or(0))
}

async fn download_remote_directory_archive(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
) -> Result<Vec<u8>, String> {
    let command = remote_directory_archive_command(remote_path);
    let output =
        run_ssh_command_for_profile_interactive(state, profile, command, String::new()).await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("打包远程目录失败。")
            .to_string());
    }
    let encoded = output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("远程目录归档解码失败：{error}"))
}

async fn download_remote_directory_archive_with_options(
    state: &AppState,
    connection_id: &str,
    profile: SshProfile,
    remote_path: &str,
    options: Option<&Value>,
) -> Result<Vec<u8>, String> {
    match download_remote_directory_archive(state, profile, remote_path).await {
        Ok(bytes) => Ok(bytes),
        Err(first_error) => {
            if !can_retry_remote_file_with_privilege(state, connection_id, options)? {
                return Err(first_error);
            }
            let encoded = run_privileged_remote_output_base64(
                state,
                connection_id,
                remote_directory_archive_command(remote_path),
                options,
                &first_error,
            )
            .await?;
            base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|error| format!("远程目录归档解码失败：{error}"))
        }
    }
}

async fn upload_local_directory_to_remote(
    state: &AppState,
    profile: SshProfile,
    local_dir: &Path,
    remote_dir: &str,
) -> Result<(), String> {
    let archive = create_tar_gz_archive(local_dir).await?;
    let remote_archive = format!("/tmp/{}.tar.gz", random_id("shelldesk-upload"));
    write_remote_file_bytes(state, profile.clone(), &remote_archive, &archive).await?;
    let command = format!(
        "mkdir -p -- {remote_dir} && tar -xzf {archive} -C {remote_dir}; code=$?; rm -f -- {archive}; exit $code",
        remote_dir = shell_quote(remote_dir),
        archive = shell_quote(&remote_archive)
    );
    let output =
        run_ssh_command_for_profile_interactive(state, profile, command, String::new()).await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("解包上传目录失败。")
            .to_string());
    }
    Ok(())
}

async fn create_tar_gz_archive(local_dir: &Path) -> Result<Vec<u8>, String> {
    let parent = local_dir
        .parent()
        .ok_or_else(|| "目录没有可用的父路径。".to_string())?;
    let name = local_dir
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| "目录名无效。".to_string())?;
    let archive_path =
        std::env::temp_dir().join(format!("{}.tar.gz", random_id("shelldesk-upload")));
    let mut command = Command::new("tar");
    prevent_tokio_process_window(&mut command);
    let output = command
        .arg("-czf")
        .arg(&archive_path)
        .arg("-C")
        .arg(parent)
        .arg(&name)
        .output()
        .await
        .map_err(error_string)?;
    if !output.status.success() {
        let _ = fs::remove_file(&archive_path);
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let bytes = fs::read(&archive_path).map_err(error_string)?;
    let _ = fs::remove_file(&archive_path);
    Ok(bytes)
}

fn extract_tar_gz_archive(bytes: &[u8], target_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(target_dir).map_err(error_string)?;
    let archive_path =
        std::env::temp_dir().join(format!("{}.tar.gz", random_id("shelldesk-download")));
    fs::write(&archive_path, bytes).map_err(error_string)?;
    let mut command = StdCommand::new("tar");
    prevent_process_window(&mut command);
    let output = command
        .arg("-xzf")
        .arg(&archive_path)
        .arg("-C")
        .arg(target_dir)
        .output()
        .map_err(error_string)?;
    let _ = fs::remove_file(&archive_path);
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

fn remote_basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("download")
        .to_string()
}

fn remote_dirname(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }
    match trimmed.rsplit_once('/') {
        Some(("", _)) => "/".to_string(),
        Some((parent, _)) if !parent.is_empty() => parent.to_string(),
        _ => ".".to_string(),
    }
}

async fn read_remote_file_bytes(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
) -> Result<Vec<u8>, String> {
    let command = remote_file_read_command(remote_path);
    let output =
        run_ssh_command_for_profile_interactive(state, profile, command, String::new()).await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("下载远程文件失败。")
            .to_string());
    }
    let encoded = output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("远程文件 base64 解码失败：{error}"))
}

async fn read_remote_file_bytes_with_options(
    state: &AppState,
    connection_id: &str,
    profile: SshProfile,
    remote_path: &str,
    options: Option<&Value>,
) -> Result<Vec<u8>, String> {
    match read_remote_file_bytes(state, profile, remote_path).await {
        Ok(bytes) => Ok(bytes),
        Err(first_error) => {
            if !can_retry_remote_file_with_privilege(state, connection_id, options)? {
                return Err(first_error);
            }
            let encoded = run_privileged_remote_output_base64(
                state,
                connection_id,
                remote_file_read_command(remote_path),
                options,
                &first_error,
            )
            .await?;
            base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|error| format!("远程文件 base64 解码失败：{error}"))
        }
    }
}

async fn write_remote_file_bytes(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let command = remote_file_write_command(remote_path);
    let output = run_ssh_command_for_profile_interactive(state, profile, command, encoded).await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("上传文件失败。")
            .to_string());
    }
    Ok(())
}

async fn write_remote_file_bytes_with_options(
    state: &AppState,
    connection_id: &str,
    profile: SshProfile,
    remote_path: &str,
    bytes: &[u8],
    options: Option<&Value>,
) -> Result<(), String> {
    match write_remote_file_bytes(state, profile, remote_path, bytes).await {
        Ok(()) => Ok(()),
        Err(first_error) => {
            if !can_retry_remote_file_with_privilege(state, connection_id, options)? {
                return Err(first_error);
            }

            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            let output = run_connection_command_with_options(
                state,
                vec![
                    json!(connection_id),
                    json!(remote_file_write_command(remote_path)),
                    json!(encoded),
                    options.cloned().unwrap_or(Value::Null),
                ],
                3,
            )
            .await?;
            if output.get("code").and_then(Value::as_i64).unwrap_or(1) == 0 {
                Ok(())
            } else {
                Err(output
                    .get("stderr")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(&first_error)
                    .to_string())
            }
        }
    }
}

async fn run_privileged_remote_output_base64(
    state: &AppState,
    connection_id: &str,
    command: String,
    options: Option<&Value>,
    fallback_error: &str,
) -> Result<String, String> {
    let output = run_connection_command_with_options(
        state,
        vec![
            json!(connection_id),
            json!(command),
            json!(""),
            options.cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) == 0 {
        return Ok(output
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string());
    }
    Err(output
        .get("stderr")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_error)
        .to_string())
}

fn can_retry_remote_file_with_privilege(
    state: &AppState,
    connection_id: &str,
    options: Option<&Value>,
) -> Result<bool, String> {
    if options
        .and_then(Value::as_object)
        .is_some_and(|object| object.contains_key("sudoPassword"))
    {
        return Ok(true);
    }
    Ok(get_connection(state, connection_id)?.privilege.is_some())
}

fn remote_file_read_command(remote_path: &str) -> String {
    format!(
        "test -f {path} && (base64 < {path} 2>/dev/null || openssl base64 -A < {path}) | tr -d '\\r\\n'",
        path = shell_quote(remote_path)
    )
}

fn remote_file_write_command(remote_path: &str) -> String {
    format!(
        r#"parent=$(dirname -- {path}); mkdir -p -- "$parent" && tmp=$(mktemp) && cat > "$tmp" && (base64 -d "$tmp" > {path} 2>/dev/null || base64 -D "$tmp" > {path} 2>/dev/null || openssl base64 -d -A -in "$tmp" -out {path}) ; code=$?; rm -f "$tmp"; exit $code"#,
        path = shell_quote(remote_path)
    )
}

fn remote_directory_archive_command(remote_path: &str) -> String {
    let parent = remote_dirname(remote_path);
    let name = remote_basename(remote_path);
    format!(
        "cd {parent} && tar -czf - -- {name} | base64 | tr -d '\\r\\n'",
        parent = shell_quote(&parent),
        name = shell_quote(&name)
    )
}

fn join_remote_path(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn remote_list_directory_command(connection: &ActiveConnection, remote_path: &str) -> String {
    if remote_host_is_windows(connection) {
        return create_powershell_command(&format!(
            r#"$target = {target}
if (-not (Test-Path -LiteralPath $target -PathType Container)) {{
  [Console]::Error.WriteLine('远程目录不存在。')
  exit 40
}}
$resolved = (Resolve-Path -LiteralPath $target).Path
$items = @(Get-ChildItem -LiteralPath $target -Force | ForEach-Object {{
  $entryType = if (($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {{ 'symlink' }} elseif ($_.PSIsContainer) {{ 'directory' }} else {{ 'file' }}
  $entrySize = if ($_.PSIsContainer) {{ 0 }} else {{ [int64]$_.Length }}
  [pscustomobject]@{{
    name = $_.Name
    longname = $_.FullName
    type = $entryType
    size = $entrySize
    mode = $(if ($_.IsReadOnly) {{ 292 }} else {{ 438 }})
    owner = 0
    group = 0
    modifiedAt = $_.LastWriteTimeUtc.ToString('o')
  }}
}})
[pscustomobject]@{{ path = $resolved; entries = $items }} | ConvertTo-Json -Compress -Depth 5"#,
            target = quote_powershell_string(remote_path)
        ));
    }
    format!(
        "find {} -maxdepth 1 -mindepth 1 -printf '%f\\t%y\\t%s\\t%T@\\n'",
        shell_quote(remote_path)
    )
}

fn remote_stat_path_command(connection: &ActiveConnection, remote_path: &str) -> String {
    if remote_host_is_windows(connection) {
        return create_powershell_command(&format!(
            r#"$target = {target}
if (-not (Test-Path -LiteralPath $target)) {{
  [Console]::Error.WriteLine('远程路径不存在。')
  exit 40
}}
$item = Get-Item -LiteralPath $target -Force
$entryType = if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {{ 'symlink' }} elseif ($item.PSIsContainer) {{ 'directory' }} else {{ 'file' }}
$entrySize = if ($item.PSIsContainer) {{ 0 }} else {{ [int64]$item.Length }}
[pscustomobject]@{{
  type = $entryType
  size = $entrySize
  mode = $(if ($item.IsReadOnly) {{ 292 }} else {{ 438 }})
  owner = 0
  group = 0
  modifiedAt = $item.LastWriteTimeUtc.ToString('o')
  accessedAt = $item.LastAccessTimeUtc.ToString('o')
}} | ConvertTo-Json -Compress -Depth 4"#,
            target = quote_powershell_string(remote_path)
        ));
    }
    format!(
        "stat -c '%F\\t%s\\t%a\\t%u\\t%g\\t%Y\\t%X' -- {}",
        shell_quote(remote_path)
    )
}

fn remote_read_file_command(connection: &ActiveConnection, remote_path: &str) -> String {
    if remote_host_is_windows(connection) {
        return create_powershell_command(&format!(
            "[Console]::Out.Write([IO.File]::ReadAllText({}))",
            quote_powershell_string(remote_path)
        ));
    }
    format!("cat -- {}", shell_quote(remote_path))
}

fn remote_write_file_command(connection: &ActiveConnection, remote_path: &str) -> String {
    if remote_host_is_windows(connection) {
        return create_powershell_command(&format!(
            r#"$target = {target}
$parent = Split-Path -LiteralPath $target -Parent
if ($parent -and -not (Test-Path -LiteralPath $parent -PathType Container)) {{
  [Console]::Error.WriteLine('远程目录不存在。')
  exit 40
}}
$content = [Console]::In.ReadToEnd()
[IO.File]::WriteAllText($target, $content, $__shelldeskUtf8)"#,
            target = quote_powershell_string(remote_path)
        ));
    }
    format!("cat > {}", shell_quote(remote_path))
}

fn remote_create_directory_command(connection: &ActiveConnection, remote_path: &str) -> String {
    if remote_host_is_windows(connection) {
        return create_powershell_command(&format!(
            "New-Item -ItemType Directory -LiteralPath {} -Force | Out-Null",
            quote_powershell_string(remote_path)
        ));
    }
    format!("mkdir -p -- {}", shell_quote(remote_path))
}

fn remote_create_file_command(connection: &ActiveConnection, remote_path: &str) -> String {
    if remote_host_is_windows(connection) {
        return create_powershell_command(&format!(
            r#"$target = {target}
$parent = Split-Path -LiteralPath $target -Parent
if ($parent -and -not (Test-Path -LiteralPath $parent -PathType Container)) {{
  [Console]::Error.WriteLine('远程目录不存在。')
  exit 40
}}
New-Item -ItemType File -LiteralPath $target -Force | Out-Null"#,
            target = quote_powershell_string(remote_path)
        ));
    }
    format!(": > {}", shell_quote(remote_path))
}

fn remote_delete_path_command(
    connection: &ActiveConnection,
    remote_path: &str,
    entry_type: &str,
) -> String {
    if remote_host_is_windows(connection) {
        return create_powershell_command(&format!(
            "Remove-Item -LiteralPath {} -Force {}",
            quote_powershell_string(remote_path),
            if entry_type == "directory" {
                "-Recurse"
            } else {
                ""
            }
        ));
    }
    if entry_type == "directory" {
        format!("rm -rf -- {}", shell_quote(remote_path))
    } else {
        format!("rm -f -- {}", shell_quote(remote_path))
    }
}

fn remote_rename_path_command(
    connection: &ActiveConnection,
    old_path: &str,
    new_path: &str,
) -> String {
    if remote_host_is_windows(connection) {
        return create_powershell_command(&format!(
            "Move-Item -LiteralPath {} -Destination {} -Force",
            quote_powershell_string(old_path),
            quote_powershell_string(new_path)
        ));
    }
    format!("mv -- {} {}", shell_quote(old_path), shell_quote(new_path))
}

fn parse_unix_directory_listing(output: Value, remote_path: String) -> Result<Value, String> {
    let stdout = command_stdout(output, "列出远程目录失败。")?;
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.split('\t');
        let name = parts.next().unwrap_or("");
        let kind = parts.next().unwrap_or("f");
        let size = parts
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let modified = parts
            .next()
            .and_then(|value| value.split('.').next())
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(|value| chrono::DateTime::<Utc>::from_timestamp(value, 0))
            .unwrap_or_else(Utc::now)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        entries.push(json!({
            "name": name,
            "longname": "",
            "type": match kind {
                "d" => "directory",
                "l" => "symlink",
                _ => "file",
            },
            "size": size,
            "modifiedAt": modified
        }));
    }
    Ok(json!({ "path": remote_path, "entries": entries }))
}

fn parse_unix_path_stat(output: Value) -> Result<Value, String> {
    let stdout = command_stdout(output, "读取远程路径属性失败。")?;
    let mut parts = stdout.trim().split('\t');
    let file_type = parts.next().unwrap_or("");
    let size = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let mode = parts
        .next()
        .and_then(|value| u32::from_str_radix(value, 8).ok())
        .unwrap_or(0);
    let owner = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let group = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let modified = parts.next().and_then(|value| value.parse::<u64>().ok());
    let accessed = parts.next().and_then(|value| value.parse::<u64>().ok());
    Ok(json!({
        "type": if file_type.contains("directory") {
            "directory"
        } else if file_type.contains("symbolic") {
            "symlink"
        } else {
            "file"
        },
        "size": size,
        "mode": mode,
        "owner": owner,
        "group": group,
        "modifiedAt": unix_time_to_iso(modified),
        "accessedAt": unix_time_to_iso(accessed)
    }))
}

fn command_stdout(output: Value, fallback_error: &str) -> Result<String, String> {
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) == 0 {
        Ok(output
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string())
    } else {
        Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback_error)
            .to_string())
    }
}

fn command_json(output: Value, fallback_error: &str) -> Result<Value, String> {
    let stdout = command_stdout(output, fallback_error)?;
    serde_json::from_str(stdout.trim()).map_err(|error| {
        format!(
            "{}：{}",
            fallback_error.trim_end_matches('。'),
            error_string(error)
        )
    })
}

fn command_bool(output: Value, fallback_error: &str) -> Result<Value, String> {
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) == 0 {
        Ok(json!(true))
    } else {
        Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback_error)
            .to_string())
    }
}

fn unix_time_to_iso(value: Option<u64>) -> String {
    chrono::DateTime::<Utc>::from_timestamp(value.unwrap_or(0) as i64, 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) async fn set_connection_path_permissions(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let remote_path = string_arg(&args, 1)?;
    let options = args.get(2).cloned().unwrap_or_else(|| json!({}));
    let mode = options.get("mode").and_then(Value::as_u64).unwrap_or(0o644);
    let recursive = options
        .get("recursive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let connection = get_connection(state, &connection_id)?;
    if connection.kind == ConnectionKind::Local {
        return set_local_path_permissions(vec![json!(connection_id), json!(remote_path), options]);
    }
    let command = if recursive {
        format!("chmod -R {:o} -- {}", mode, shell_quote(&remote_path))
    } else {
        format!("chmod {:o} -- {}", mode, shell_quote(&remote_path))
    };
    let output = run_connection_command_with_options(
        state,
        vec![json!(connection_id), json!(command), json!(""), options],
        3,
    )
    .await?;
    command_bool(output, "修改权限失败。")
}

pub(crate) async fn compress_connection_paths(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let source_paths = args
        .get(1)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let requested_format = string_arg(&args, 2).unwrap_or_else(|_| "zip".to_string());
    let format = normalize_archive_format(&requested_format);
    let dest_path = string_arg(&args, 3)?;
    let sources: Vec<String> = source_paths
        .iter()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect();
    if sources.is_empty() {
        return Err("请选择要压缩的路径。".to_string());
    }
    let connection = get_connection(state, &connection_id)?;
    let command = archive_compress_command(&connection, &sources, &format, &dest_path)?;
    let output = if connection.kind == ConnectionKind::Local {
        run_shell(command, "", Duration::from_secs(300)).await?
    } else {
        run_ssh_command_for_profile_interactive(
            state,
            connection
                .ssh
                .ok_or_else(|| "SSH profile is unavailable.".to_string())?,
            command,
            String::new(),
        )
        .await?
    };
    command_bool(output, "压缩失败。")?;
    Ok(json!({ "format": format, "destPath": dest_path }))
}

pub(crate) async fn decompress_connection_archive(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let archive_path = string_arg(&args, 1)?;
    let dest_dir = args
        .get(2)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(".")
        .to_string();
    let connection = get_connection(state, &connection_id)?;
    let command = archive_decompress_command(&connection, &archive_path, &dest_dir)?;
    let output = if connection.kind == ConnectionKind::Local {
        run_shell(command, "", Duration::from_secs(300)).await?
    } else {
        run_ssh_command_for_profile_interactive(
            state,
            connection
                .ssh
                .ok_or_else(|| "SSH profile is unavailable.".to_string())?,
            command,
            String::new(),
        )
        .await?
    };
    command_bool(output, "解压失败。")?;
    Ok(json!({ "archivePath": archive_path, "destDir": dest_dir }))
}

fn normalize_archive_format(format: &str) -> String {
    match format {
        "zip" | "tar" | "tar.gz" | "tgz" | "7z" => format.to_string(),
        _ => "zip".to_string(),
    }
}

fn archive_compress_command(
    connection: &ActiveConnection,
    source_paths: &[String],
    format: &str,
    dest_path: &str,
) -> Result<String, String> {
    if remote_host_is_windows(connection) {
        if format != "zip" {
            return Err("Windows 主机暂仅支持 ZIP 压缩。".to_string());
        }
        return Ok(create_powershell_command(&format!(
            "Compress-Archive -LiteralPath @({}) -DestinationPath {} -Force",
            source_paths
                .iter()
                .map(|value| quote_powershell_string(value))
                .collect::<Vec<_>>()
                .join(", "),
            quote_powershell_string(dest_path)
        )));
    }

    let escaped_sources = source_paths
        .iter()
        .map(|value| shell_quote(value))
        .collect::<Vec<_>>()
        .join(" ");
    let escaped_dest = shell_quote(dest_path);
    let command = match format {
        "zip" => format!("zip -r -- {escaped_dest} {escaped_sources}"),
        "tar" => format!("tar cf {escaped_dest} -- {escaped_sources}"),
        "tar.gz" | "tgz" => format!("tar czf {escaped_dest} -- {escaped_sources}"),
        "7z" => format!("7z a {escaped_dest} {escaped_sources}"),
        _ => format!("zip -r -- {escaped_dest} {escaped_sources}"),
    };
    Ok(command)
}

fn archive_decompress_command(
    connection: &ActiveConnection,
    archive_path: &str,
    dest_dir: &str,
) -> Result<String, String> {
    let archive_name = remote_basename(archive_path).to_lowercase();
    if remote_host_is_windows(connection) {
        if !archive_name.ends_with(".zip") {
            return Err("Windows 主机暂仅支持 ZIP 解压缩。".to_string());
        }
        return Ok(create_powershell_command(&format!(
            "Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
            quote_powershell_string(archive_path),
            quote_powershell_string(dest_dir)
        )));
    }

    let escaped_archive = shell_quote(archive_path);
    let escaped_dest = shell_quote(dest_dir);
    if archive_name.ends_with(".tar.gz") || archive_name.ends_with(".tgz") {
        Ok(format!(
            "mkdir -p -- {escaped_dest} && tar xzf {escaped_archive} -C {escaped_dest}"
        ))
    } else if archive_name.ends_with(".tar.bz2") || archive_name.ends_with(".tbz2") {
        Ok(format!(
            "mkdir -p -- {escaped_dest} && tar xjf {escaped_archive} -C {escaped_dest}"
        ))
    } else if archive_name.ends_with(".tar.xz") || archive_name.ends_with(".txz") {
        Ok(format!(
            "mkdir -p -- {escaped_dest} && tar xJf {escaped_archive} -C {escaped_dest}"
        ))
    } else if archive_name.ends_with(".tar") {
        Ok(format!(
            "mkdir -p -- {escaped_dest} && tar xf {escaped_archive} -C {escaped_dest}"
        ))
    } else if archive_name.ends_with(".zip") {
        Ok(format!(
            "mkdir -p -- {escaped_dest} && unzip -o -- {escaped_archive} -d {escaped_dest}"
        ))
    } else if archive_name.ends_with(".7z") {
        Ok(format!(
            "mkdir -p -- {escaped_dest} && 7z x -o{escaped_dest} {escaped_archive} -y"
        ))
    } else if archive_name.ends_with(".gz") && !archive_name.ends_with(".tar.gz") {
        let base_name = remote_basename(archive_path)
            .trim_end_matches(".gz")
            .to_string();
        Ok(format!(
            "mkdir -p -- {escaped_dest} && gunzip -c {escaped_archive} > {}/{}",
            escaped_dest,
            shell_quote(&base_name)
        ))
    } else if archive_name.ends_with(".rar") {
        Ok(format!(
            "mkdir -p -- {escaped_dest} && unrar x -o+ {escaped_archive} {escaped_dest}"
        ))
    } else {
        Err(format!(
            "不支持的压缩格式：{}",
            remote_basename(archive_path)
        ))
    }
}

fn remote_host_is_windows(connection: &ActiveConnection) -> bool {
    connection
        .host
        .get("systemType")
        .and_then(Value::as_str)
        .is_some_and(|system_type| system_type.eq_ignore_ascii_case("windows"))
}

fn quote_powershell_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn create_powershell_command(script: &str) -> String {
    let prelude = [
        "try {",
        "$__shelldeskUtf8 = New-Object System.Text.UTF8Encoding $false",
        "[Console]::InputEncoding = $__shelldeskUtf8",
        "[Console]::OutputEncoding = $__shelldeskUtf8",
        "$OutputEncoding = $__shelldeskUtf8",
        "} catch {}",
        "try { chcp.com 65001 > $null } catch {}",
    ]
    .join("\n");
    let encoded = base64::engine::general_purpose::STANDARD
        .encode(utf16le_bytes(&format!("{prelude}\n{script}")));
    format!("powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand {encoded}")
}

fn utf16le_bytes(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActiveConnection, ActiveTransfer, PrivilegeConfig};
    use std::collections::HashSet;

    fn test_ssh_connection(privilege: Option<PrivilegeConfig>) -> ActiveConnection {
        ActiveConnection {
            id: "conn-1".to_string(),
            kind: ConnectionKind::Ssh,
            partition: "persist:conn-1".to_string(),
            proxy_port: 0,
            browser_certificate_trust: HashSet::new(),
            connected_at: "now".to_string(),
            host: json!({ "systemType": "linux" }),
            ssh: Some(SshProfile {
                address: "example.test".to_string(),
                port: 22,
                username: "user".to_string(),
                auth_method: "password".to_string(),
                password: "secret".to_string(),
                key_path: String::new(),
                known_hosts_path: String::new(),
                proxy_helper_exe: String::new(),
                proxy: None,
                jump: None,
            }),
            privilege,
        }
    }

    fn test_windows_connection() -> ActiveConnection {
        let mut connection = test_ssh_connection(None);
        connection.host = json!({ "systemType": "windows" });
        connection
    }

    #[test]
    fn remote_file_name_handles_unix_and_windows_paths() {
        assert_eq!(
            remote_file_name("/var/log/nginx/access.log", "download"),
            "access.log"
        );
        assert_eq!(
            remote_file_name("C:\\Logs\\app\\error.log", "download"),
            "error.log"
        );
        assert_eq!(remote_file_name("/", "download"), "download");
    }

    #[test]
    fn local_download_file_name_matches_legacy_sanitization() {
        assert_eq!(
            sanitize_local_file_name("logs/../prod:dump?.txt ", "download"),
            "logs_.._prod_dump_.txt"
        );
        assert_eq!(sanitize_local_file_name(" .hidden ", "download"), ".hidden");
        assert_eq!(sanitize_local_file_name("CON.txt", "download"), "download");
        assert_eq!(sanitize_local_file_name("...", "download"), "download");
    }

    #[test]
    fn upload_remote_name_matches_legacy_sanitization() {
        let local_path = PathBuf::from("C:\\Users\\me\\report.txt");

        assert_eq!(
            upload_remote_name(
                &json!({ "remoteName": "../prod:dump?.txt " }),
                local_path.as_path()
            ),
            ".._prod_dump_.txt"
        );
        assert_eq!(
            upload_remote_name(&json!({ "remoteName": "CON.txt" }), local_path.as_path()),
            "upload"
        );
        assert_eq!(
            upload_remote_name(&json!({ "remoteName": "" }), local_path.as_path()),
            "report.txt"
        );
    }

    #[test]
    fn cancel_transfer_matches_active_queue_or_client_id() {
        let state = AppState::new(std::env::temp_dir());
        state.active_transfers.lock().unwrap().insert(
            "queue-1".to_string(),
            ActiveTransfer {
                connection_id: "conn-1".to_string(),
                client_id: Some("client-1".to_string()),
            },
        );

        assert_eq!(
            cancel_transfer(&state, vec![json!("conn-2"), json!("queue-1")]).unwrap(),
            json!(false)
        );
        assert_eq!(
            cancel_transfer(&state, vec![json!("conn-1"), json!("client-1")]).unwrap(),
            json!(true)
        );

        let cancellations = state.transfer_cancellations.lock().unwrap();
        assert!(cancellations.contains("queue-1"));
        assert!(cancellations.contains("client-1"));
    }

    #[test]
    fn cancel_transfer_without_queue_cancels_all_connection_transfers() {
        let state = AppState::new(std::env::temp_dir());
        {
            let mut active_transfers = state.active_transfers.lock().unwrap();
            active_transfers.insert(
                "queue-1".to_string(),
                ActiveTransfer {
                    connection_id: "conn-1".to_string(),
                    client_id: Some("client-1".to_string()),
                },
            );
            active_transfers.insert(
                "queue-2".to_string(),
                ActiveTransfer {
                    connection_id: "conn-2".to_string(),
                    client_id: Some("client-2".to_string()),
                },
            );
        }

        assert_eq!(
            cancel_transfer(&state, vec![json!("conn-1"), json!("")]).unwrap(),
            json!(true)
        );

        let cancellations = state.transfer_cancellations.lock().unwrap();
        assert!(cancellations.contains("queue-1"));
        assert!(cancellations.contains("client-1"));
        assert!(!cancellations.contains("queue-2"));
        assert!(!cancellations.contains("client-2"));
    }

    #[test]
    fn windows_remote_file_commands_use_powershell() {
        let connection = test_windows_connection();

        for command in [
            remote_list_directory_command(&connection, "C:\\Logs"),
            remote_stat_path_command(&connection, "C:\\Logs\\app.log"),
            remote_read_file_command(&connection, "C:\\Logs\\app.log"),
            remote_write_file_command(&connection, "C:\\Logs\\app.log"),
            remote_create_directory_command(&connection, "C:\\Logs\\new"),
            remote_create_file_command(&connection, "C:\\Logs\\new.txt"),
            remote_delete_path_command(&connection, "C:\\Logs\\new", "directory"),
            remote_rename_path_command(&connection, "C:\\Logs\\old.txt", "C:\\Logs\\new.txt"),
        ] {
            assert!(command
                .starts_with("powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand "));
        }
    }

    #[test]
    fn unix_remote_file_commands_keep_shell_semantics() {
        let connection = test_ssh_connection(None);

        assert!(remote_list_directory_command(&connection, "/var/log").contains("find '/var/log'"));
        assert!(remote_stat_path_command(&connection, "/var/log/app.log").contains("stat -c"));
        assert_eq!(
            remote_read_file_command(&connection, "/var/log/app.log"),
            "cat -- '/var/log/app.log'"
        );
        assert_eq!(
            remote_rename_path_command(&connection, "/tmp/a", "/tmp/b"),
            "mv -- '/tmp/a' '/tmp/b'"
        );
    }

    #[test]
    fn sftp_probe_command_does_not_mask_missing_server() {
        let command = remote_sftp_probe_command();

        assert!(command.contains("command -v sftp-server"));
        assert!(command.contains("/usr/lib/openssh/sftp-server"));
        assert!(!command.contains("|| true"));
    }

    #[test]
    fn parses_unix_directory_and_stat_outputs() {
        let listing = parse_unix_directory_listing(
            json!({ "code": 0, "stdout": "app.log\tf\t12\t1710000000.0\nlogs\td\t0\t1710000010.0\n" }),
            "/var".to_string(),
        )
        .unwrap();
        assert_eq!(listing["entries"][0]["name"], "app.log");
        assert_eq!(listing["entries"][1]["type"], "directory");

        let stat = parse_unix_path_stat(json!({
            "code": 0,
            "stdout": "regular file\t12\t644\t1000\t1000\t1710000000\t1710000001\n"
        }))
        .unwrap();
        assert_eq!(stat["type"], "file");
        assert_eq!(stat["mode"], 0o644);
    }

    #[test]
    fn retries_remote_file_operation_when_sudo_password_is_supplied() {
        let state = AppState::new(std::env::temp_dir());
        let options = json!({ "sudoPassword": "secret" });

        assert!(can_retry_remote_file_with_privilege(&state, "missing", Some(&options)).unwrap());
    }

    #[test]
    fn retries_remote_file_operation_when_connection_has_su_root_privilege() {
        let state = AppState::new(std::env::temp_dir());
        state.connections.lock().unwrap().insert(
            "conn-1".to_string(),
            test_ssh_connection(Some(PrivilegeConfig {
                mode: "su-root".to_string(),
                password: "root-pass".to_string(),
            })),
        );

        assert!(can_retry_remote_file_with_privilege(&state, "conn-1", None).unwrap());
    }

    #[test]
    fn remote_file_write_command_quotes_target_path() {
        let command = remote_file_write_command("/etc/app's/config.ini");

        assert!(command.contains("'/etc/app'\"'\"'s/config.ini'"));
        assert!(!command.contains("/etc/app's/config.ini >"));
    }

    #[test]
    fn remote_file_read_command_quotes_target_path() {
        let command = remote_file_read_command("/var/lib/app's/data.db");

        assert!(command.contains("'/var/lib/app'\"'\"'s/data.db'"));
        assert!(!command.contains("< /var/lib/app's/data.db"));
    }

    #[test]
    fn remote_directory_archive_command_quotes_parent_and_name() {
        let command = remote_directory_archive_command("/srv/app's/log dir");

        assert!(command.contains("cd '/srv/app'\"'\"'s'"));
        assert!(command.contains("-- 'log dir'"));
    }

    #[test]
    fn archive_compress_command_preserves_tar_format() {
        let connection = test_ssh_connection(None);
        let command = archive_compress_command(
            &connection,
            &["/var/log/app".to_string()],
            "tar",
            "/tmp/logs.tar",
        )
        .unwrap();

        assert!(command.starts_with("tar cf "));
        assert!(!command.contains("tar czf"));
    }

    #[test]
    fn archive_decompress_command_supports_tar_xz_and_gz() {
        let connection = test_ssh_connection(None);

        let tar_xz =
            archive_decompress_command(&connection, "/tmp/archive.tar.xz", "/opt/out").unwrap();
        assert!(tar_xz.contains("tar xJf '/tmp/archive.tar.xz'"));

        let gz = archive_decompress_command(&connection, "/tmp/access.log.gz", "/opt/out").unwrap();
        assert!(gz.contains("gunzip -c '/tmp/access.log.gz'"));
        assert!(gz.contains("> '/opt/out'/'access.log'"));
    }

    #[test]
    fn archive_command_uses_powershell_for_windows_zip() {
        let connection = test_windows_connection();
        let command = archive_compress_command(
            &connection,
            &["C:\\Logs\\app.log".to_string()],
            "zip",
            "C:\\Logs\\app.zip",
        )
        .unwrap();

        assert!(
            command.starts_with("powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand ")
        );
        assert!(archive_compress_command(
            &connection,
            &["C:\\Logs".to_string()],
            "tar",
            "C:\\Logs.tar"
        )
        .is_err());
        assert!(archive_decompress_command(&connection, "C:\\Logs\\app.7z", "C:\\Logs").is_err());
    }
}

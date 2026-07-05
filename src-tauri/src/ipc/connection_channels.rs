#[path = "database_channels.rs"]
mod database_channels;

use crate::{
    browser_proxy, connection, connection_monitor, http_tunnel, remote_fs, run_connection_command,
    run_connection_command_stream, terminal, vnc, zmodem, AppState,
};
use serde_json::{json, Value};
use tauri::Manager;

pub(crate) async fn dispatch(
    state: AppState,
    window: tauri::Window,
    channel: String,
    args: Vec<Value>,
) -> Option<Result<Value, String>> {
    if database_channels::is_database_channel(&channel) {
        return dispatch_database(state.clone(), window.clone(), channel, args).await;
    }
    let result = match channel.as_str() {
        "connection:connect" => connection::connect_ssh(state.clone(), window.clone(), args).await,
        "connection:open-local" => connection::open_local_connection(&state),
        "connection:host-key-response" => {
            connection::respond_host_key_verification(&state, window.app_handle(), args)
        }
        "connection:keyboard-interactive-response" => {
            connection::respond_keyboard_interactive(&state, args)
        }
        "connection:trust-browser-certificate" => {
            connection::trust_browser_certificate(&state, args)
        }
        "connection:browser-resolve-url" => {
            browser_proxy::browser_resolve_url(state.clone(), window.clone(), args).await
        }
        "connection:get-ipc-capabilities" => {
            Ok(json!({ "terminalSessions": true, "terminalBinary": true }))
        }
        "connection:disconnect" => connection::disconnect_connection(&state, &window, args),
        "connection:get-info" => connection::get_connection_info(&state, args),
        "connection:get-status" => dispatch_monitor(state.clone(), channel.clone(), args).await,
        "connection:get-system-info" => {
            dispatch_monitor(state.clone(), channel.clone(), args).await
        }
        "connection:get-metrics" => dispatch_monitor(state.clone(), channel.clone(), args).await,
        "connection:start-terminal" => terminal::start_terminal(state.clone(), window, args).await,
        "connection:write-terminal" => terminal::write_terminal(&state, args),
        "connection:write-terminal-binary" => terminal::write_terminal_bytes(&state, args),
        "connection:resize-terminal" => terminal::resize_terminal(&state, args),
        "connection:close-terminal" => terminal::close_terminal(&state, args),
        "connection:run-command" => {
            dispatch_run_command(state.clone(), None, channel.clone(), args).await
        }
        "connection:run-command-stream" => {
            dispatch_run_command(state.clone(), Some(window), channel.clone(), args).await
        }
        "connection:http-tunnel-get" => http_tunnel::get(state.clone(), window.clone(), args).await,
        "connection:http-tunnel-post" => {
            http_tunnel::post(state.clone(), window.clone(), args).await
        }
        "connection:http-tunnel-put" => http_tunnel::put(state.clone(), window.clone(), args).await,
        "connection:http-tunnel-delete" => {
            http_tunnel::delete(state.clone(), window.clone(), args).await
        }
        "connection:list-directory"
        | "connection:stat-path"
        | "connection:read-file"
        | "connection:write-file"
        | "connection:create-directory"
        | "connection:create-file"
        | "connection:delete-path"
        | "connection:rename-path"
        | "connection:set-path-permissions"
        | "connection:check-sftp"
        | "connection:compress"
        | "connection:decompress" => {
            dispatch_remote_fs(state.clone(), None, channel.clone(), args).await
        }
        "connection:select-upload-files" => remote_fs::select_upload_items(false),
        "connection:select-upload-folders" => remote_fs::select_upload_items(true),
        "connection:download-file"
        | "connection:download-paths"
        | "connection:upload-local-paths"
        | "connection:upload-file"
        | "connection:upload-files"
        | "connection:upload-paths" => {
            dispatch_remote_fs(state.clone(), Some(window.clone()), channel.clone(), args).await
        }
        "connection:cancel-transfer" => remote_fs::cancel_transfer(&state, args),
        "connection:zmodem-select-upload-files" => zmodem::select_zmodem_upload_files(&state),
        "connection:zmodem-read-upload-file" => zmodem::read_zmodem_upload_file(&state, args),
        "connection:zmodem-release-upload-files" => {
            zmodem::release_zmodem_upload_files(&state, args)
        }
        "connection:zmodem-save-file" => zmodem::save_zmodem_file(args),
        "connection:vnc-probe" => vnc::probe(state.clone(), window.clone(), args).await,
        "connection:vnc-start" => vnc::start(state.clone(), window.clone(), args).await,
        "connection:vnc-stop" => vnc::stop(&state, args),
        _ => return None,
    };

    Some(result)
}

async fn dispatch_database(
    state: AppState,
    window: tauri::Window,
    channel: String,
    args: Vec<Value>,
) -> Option<Result<Value, String>> {
    match tokio::task::spawn_blocking(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => return Some(Err(crate::error_string(error))),
        };
        runtime.block_on(database_channels::dispatch(state, window, channel, args))
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Some(Err(crate::error_string(error))),
    }
}

async fn dispatch_monitor(
    state: AppState,
    channel: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(crate::error_string)?;
        runtime.block_on(async move {
            match channel.as_str() {
                "connection:get-status" => {
                    connection_monitor::get_connection_status(state, args).await
                }
                "connection:get-system-info" => {
                    connection_monitor::get_connection_system_info(state, args).await
                }
                "connection:get-metrics" => {
                    connection_monitor::get_connection_metrics(state, args).await
                }
                _ => Err(format!("Unsupported monitor IPC channel: {channel}")),
            }
        })
    })
    .await
    .map_err(crate::error_string)?
}

async fn dispatch_run_command(
    state: AppState,
    window: Option<tauri::Window>,
    channel: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(crate::error_string)?;
        runtime.block_on(async move {
            match channel.as_str() {
                "connection:run-command" => run_connection_command(state, args).await,
                "connection:run-command-stream" => {
                    let window = window.ok_or_else(|| "命令输出窗口不可用。".to_string())?;
                    run_connection_command_stream(state, window, args).await
                }
                _ => Err(format!("Unsupported command IPC channel: {channel}")),
            }
        })
    })
    .await
    .map_err(crate::error_string)?
}

async fn dispatch_remote_fs(
    state: AppState,
    window: Option<tauri::Window>,
    channel: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(crate::error_string)?;
        runtime.block_on(async move {
            match channel.as_str() {
                "connection:list-directory" => {
                    remote_fs::list_connection_directory(state, args).await
                }
                "connection:stat-path" => remote_fs::stat_connection_path(state, args).await,
                "connection:read-file" => remote_fs::read_connection_file(state, args).await,
                "connection:write-file" => remote_fs::write_connection_file(state, args).await,
                "connection:create-directory" => {
                    remote_fs::create_connection_directory(state, args).await
                }
                "connection:create-file" => remote_fs::create_connection_file(state, args).await,
                "connection:delete-path" => remote_fs::delete_connection_path(state, args).await,
                "connection:rename-path" => remote_fs::rename_connection_path(state, args).await,
                "connection:set-path-permissions" => {
                    remote_fs::set_connection_path_permissions(state, args).await
                }
                "connection:check-sftp" => remote_fs::check_connection_sftp(state, args).await,
                "connection:compress" => remote_fs::compress_connection_paths(state, args).await,
                "connection:decompress" => {
                    remote_fs::decompress_connection_archive(state, args).await
                }
                "connection:download-file" => {
                    let window = window.ok_or_else(|| "文件传输窗口不可用。".to_string())?;
                    remote_fs::download_connection_file(state, window, args).await
                }
                "connection:download-paths" => {
                    let window = window.ok_or_else(|| "文件传输窗口不可用。".to_string())?;
                    remote_fs::download_connection_paths(state, window, args).await
                }
                "connection:upload-local-paths" => {
                    let window = window.ok_or_else(|| "文件传输窗口不可用。".to_string())?;
                    remote_fs::upload_connection_paths(state, window, args).await
                }
                "connection:upload-file" => {
                    let window = window.ok_or_else(|| "文件传输窗口不可用。".to_string())?;
                    remote_fs::upload_selected_paths(state, window, args, false, false).await
                }
                "connection:upload-files" => {
                    let window = window.ok_or_else(|| "文件传输窗口不可用。".to_string())?;
                    remote_fs::upload_selected_paths(state, window, args, false, true).await
                }
                "connection:upload-paths" => {
                    let window = window.ok_or_else(|| "文件传输窗口不可用。".to_string())?;
                    remote_fs::upload_selected_paths(state, window, args, true, true).await
                }
                _ => Err(format!("Unsupported file IPC channel: {channel}")),
            }
        })
    })
    .await
    .map_err(crate::error_string)?
}

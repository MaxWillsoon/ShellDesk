use crate::updater::{
    check_for_update_download, check_release_info, download_update, install_update,
    read_update_state,
};
use crate::{app as app_handlers, error_string, tray, AppState};
use serde_json::{json, Value};
use tauri::Emitter;

pub(crate) async fn dispatch(
    app: tauri::AppHandle,
    window: tauri::Window,
    state: AppState,
    channel: String,
    args: &[Value],
) -> Result<Option<Value>, String> {
    let value = match channel.as_str() {
        "app:get-info" => json!(app_handlers::get_info(&app)),
        "app:open-external" => app_handlers::open_external(args.to_vec())?,
        "app:open-connection-window" => {
            app_handlers::open_connection_window(&app, &state, args.to_vec())?
        }
        "app:open-main-ai-settings" => app_handlers::open_main_ai_settings(&app)?,
        "app:check-for-updates" => check_release_info(app.clone()).await?,
        "app:get-update-status" => read_update_state(&state, &app),
        "app:check-for-update-download" => {
            check_for_update_download(state.clone(), window.clone(), app.clone()).await?
        }
        "app:download-update" => {
            download_update(state.clone(), window.clone(), app.clone()).await?
        }
        "app:install-update" => install_update(state.clone(), app.clone()).await?,

        "window:show" => {
            window.show().map_err(error_string)?;
            Value::Null
        }
        "window:minimize" => {
            window.minimize().map_err(error_string)?;
            Value::Null
        }
        "window:toggle-maximize" => {
            let maximized = window.is_maximized().map_err(error_string)?;
            if maximized {
                window.unmaximize().map_err(error_string)?;
                let _ = window.emit(
                    "window:maximize-state-changed",
                    json!({ "maximized": false }),
                );
                json!(false)
            } else {
                window.maximize().map_err(error_string)?;
                let _ = window.emit(
                    "window:maximize-state-changed",
                    json!({ "maximized": true }),
                );
                json!(true)
            }
        }
        "window:is-maximized" => json!(window.is_maximized().map_err(error_string)?),
        "window:close" => {
            tray::close_window(&window, &state)?;
            Value::Null
        }
        _ => return Ok(None),
    };

    Ok(Some(value))
}

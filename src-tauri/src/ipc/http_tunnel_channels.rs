use crate::{http_tunnel, AppState};
use serde_json::Value;

pub(crate) async fn dispatch(
    state: &AppState,
    window: &tauri::Window,
    channel: &str,
    args: &[Value],
) -> Result<Option<Value>, String> {
    let value = match channel {
        "http_tunnel_get" => http_tunnel::get(state, window, args.to_vec()).await?,
        "http_tunnel_post" => http_tunnel::post(state, window, args.to_vec()).await?,
        "http_tunnel_put" => http_tunnel::put(state, window, args.to_vec()).await?,
        "http_tunnel_delete" => http_tunnel::delete(state, window, args.to_vec()).await?,
        _ => return Ok(None),
    };

    Ok(Some(value))
}

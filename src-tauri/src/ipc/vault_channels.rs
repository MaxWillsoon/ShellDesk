use crate::vault::{
    generate_key_pair, get_bookmarks, get_preference, get_remote_connection_profile,
    import_key_pair, public_snapshot, read_store, save_bookmarks_to_store,
    save_remote_connection_profile_to_store, set_preference_to_store, snapshot, to_snapshot,
    upsert_vault_collections, write_store,
};
use crate::{string_arg, AppState};
use serde_json::{json, Value};
use tauri::Emitter;

pub(crate) async fn dispatch(
    state: &AppState,
    window: &tauri::Window,
    channel: &str,
    args: &[Value],
) -> Result<Option<Value>, String> {
    let value = match channel {
        "vault:get-public-snapshot" => public_snapshot(state)?,
        "vault:get-snapshot" => snapshot(state)?,
        "vault:save-collections" => {
            let payload = args.first().cloned().unwrap_or(Value::Null);
            let mut store = read_store(state)?;
            upsert_vault_collections(&mut store, payload)?;
            write_store(state, &store)?;
            let _ = window.emit("vault:changed", json!({ "kind": "vault" }));
            to_snapshot(state, store)
        }
        "vault:get-bookmarks" => {
            let scope = string_arg(args, 0)?;
            let store = read_store(state)?;
            get_bookmarks(&store, &scope)?
        }
        "vault:save-bookmarks" => {
            let scope = string_arg(args, 0)?;
            let bookmarks = args.get(1).cloned().unwrap_or_else(|| json!([]));
            let mut store = read_store(state)?;
            let bookmarks = save_bookmarks_to_store(&mut store, &scope, bookmarks)?;
            write_store(state, &store)?;
            let _ = window.emit(
                "vault:changed",
                json!({ "kind": "bookmarks", "scope": scope }),
            );
            bookmarks
        }
        "vault:get-remote-connection-profile" => {
            let host_id = string_arg(args, 0)?;
            let app_key = string_arg(args, 1)?;
            let store = read_store(state)?;
            get_remote_connection_profile(&store, &host_id, &app_key)?
        }
        "vault:save-remote-connection-profile" => {
            let host_id = string_arg(args, 0)?;
            let app_key = string_arg(args, 1)?;
            let values = args.get(2).cloned().unwrap_or_else(|| json!({}));
            let mut store = read_store(state)?;
            let values =
                save_remote_connection_profile_to_store(&mut store, &host_id, &app_key, values)?;
            write_store(state, &store)?;
            values
        }
        "vault:import-key-pair" => import_key_pair(state, window, args.to_vec()).await?,
        "vault:generate-rsa-key-pair" => generate_key_pair(state, window, args.to_vec()).await?,

        "preferences:get" => {
            let key = string_arg(args, 0)?;
            let store = read_store(state)?;
            get_preference(&store, &key)?
        }
        "preferences:set" => {
            let key = string_arg(args, 0)?;
            let value = args.get(1).cloned().unwrap_or(Value::Null);
            let mut store = read_store(state)?;
            let value = set_preference_to_store(&mut store, &key, value)?;
            write_store(state, &store)?;
            let _ = window.emit("vault:changed", json!({ "kind": "preference", "key": key }));
            value
        }
        _ => return Ok(None),
    };

    Ok(Some(value))
}

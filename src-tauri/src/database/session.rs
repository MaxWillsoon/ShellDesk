use serde_json::{json, Value};

use crate::{error_string, string_arg, AppState};

pub(super) fn register_db_session(
    state: &AppState,
    kind: &str,
    connection_id: &str,
    session_id: &str,
    config: Value,
) -> Result<(), String> {
    state
        .database_sessions
        .lock()
        .map_err(error_string)?
        .insert(db_session_key(kind, connection_id, session_id), config);
    Ok(())
}

pub(crate) fn disconnect_db_session(
    state: &AppState,
    args: Vec<Value>,
    kind: &str,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let session_id = string_arg(&args, 1)?;
    state
        .database_sessions
        .lock()
        .map_err(error_string)?
        .remove(&db_session_key(kind, &connection_id, &session_id));
    Ok(json!(true))
}

pub(super) fn decode_active_db_session_args(
    state: &AppState,
    kind: &str,
    args: &[Value],
    connection_index: usize,
    session_index: usize,
) -> Result<(String, Value), String> {
    let connection_id = string_arg(args, connection_index)?;
    let session_id = string_arg(args, session_index)?;
    let key = db_session_key(kind, &connection_id, &session_id);
    let config = state
        .database_sessions
        .lock()
        .map_err(error_string)?
        .get(&key)
        .cloned()
        .ok_or_else(|| format!("{} 连接已断开。", db_display_name(kind)))?;
    Ok((connection_id, config))
}

fn db_session_key(kind: &str, connection_id: &str, session_id: &str) -> String {
    format!("{kind}:{connection_id}:{session_id}")
}

fn db_display_name(kind: &str) -> &'static str {
    match kind {
        "mysql" => "MySQL",
        "postgres" => "PostgreSQL",
        "redis" => "Redis",
        "sqlite" => "SQLite",
        "clickhouse" => "ClickHouse",
        "mongo" => "MongoDB",
        _ => "数据库",
    }
}

use serde_json::{json, Value};

use super::{
    codec::{encode_config_id, url_encode},
    parse::parse_clickhouse_response,
    session::{decode_active_db_session_args, register_db_session},
    should_fallback_to_database_cli, should_try_database_tunnel,
    sql::{clickhouse_literal, clickhouse_query_with_json_format},
    tunnel,
};
use crate::{ps_quote, read_string_field, run_cli_output, shell_quote, string_arg, AppState};

pub(crate) async fn clickhouse_connect(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let config = args.get(1).cloned().unwrap_or_else(|| json!({}));
    let mut fallback_reason = None;
    if should_try_database_tunnel(state, &connection_id, &config)? {
        match tunnel::clickhouse_connect(state, window, args.clone()).await {
            Ok(result) => return Ok(result),
            Err(error) if should_fallback_to_database_cli(&config) => {
                eprintln!(
                    "[database] ClickHouse SSH tunnel unavailable, falling back to CLI: {error}"
                );
                fallback_reason = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    let clickhouse_id = encode_config_id("clickhouse", &config)?;
    let _ = run_clickhouse_query(state, &connection_id, &config, "SELECT 1 AS ok", None).await?;
    register_db_session(state, "clickhouse", &connection_id, &clickhouse_id, config)?;
    Ok(json!({
        "clickhouseId": clickhouse_id,
        "transport": "ssh-exec",
        "fallbackReason": fallback_reason,
    }))
}

pub(crate) async fn clickhouse_databases(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    if tunnel::has_session(state, "clickhouse", &args)? {
        return tunnel::clickhouse_databases(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "clickhouse", &args, 0, 1)?;
    let result = run_clickhouse_query(
        state,
        &connection_id,
        &config,
        "SELECT name FROM system.databases ORDER BY name",
        None,
    )
    .await?;
    Ok(json!(result
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| row
            .get("name")
            .and_then(Value::as_str)
            .map(ToString::to_string))
        .collect::<Vec<_>>()))
}

pub(crate) async fn clickhouse_tables(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    if tunnel::has_session(state, "clickhouse", &args)? {
        return tunnel::clickhouse_tables(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "clickhouse", &args, 0, 1)?;
    let database = string_arg(&args, 2)?;
    let sql = format!(
        "SELECT name, engine, total_rows AS totalRows, total_bytes AS totalBytes FROM system.tables WHERE database = {} ORDER BY name",
        clickhouse_literal(&database)
    );
    let result = run_clickhouse_query(state, &connection_id, &config, &sql, None).await?;
    Ok(result.get("rows").cloned().unwrap_or_else(|| json!([])))
}

pub(crate) async fn clickhouse_columns(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    if tunnel::has_session(state, "clickhouse", &args)? {
        return tunnel::clickhouse_columns(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "clickhouse", &args, 0, 1)?;
    let database = string_arg(&args, 2)?;
    let table = string_arg(&args, 3)?;
    let sql = format!(
        "SELECT name, type, default_kind AS defaultKind, default_expression AS defaultExpression, comment, is_in_primary_key AS isPrimaryKey, is_in_sorting_key AS isSortingKey FROM system.columns WHERE database = {} AND table = {} ORDER BY position",
        clickhouse_literal(&database),
        clickhouse_literal(&table)
    );
    let result = run_clickhouse_query(state, &connection_id, &config, &sql, None).await?;
    Ok(result.get("rows").cloned().unwrap_or_else(|| json!([])))
}

pub(crate) async fn clickhouse_query(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    if tunnel::has_session(state, "clickhouse", &args)? {
        return tunnel::clickhouse_query(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "clickhouse", &args, 0, 1)?;
    let sql = string_arg(&args, 2)?;
    let database = args.get(3).and_then(Value::as_str);
    run_clickhouse_query(state, &connection_id, &config, &sql, database).await
}

async fn run_clickhouse_query(
    state: &AppState,
    connection_id: &str,
    config: &Value,
    sql: &str,
    database_override: Option<&str>,
) -> Result<Value, String> {
    let host = read_string_field(config, "host", "127.0.0.1");
    let secure = config
        .get("secure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let port = config
        .get("port")
        .and_then(Value::as_u64)
        .unwrap_or(if secure { 8443 } else { 8123 });
    let scheme = if secure { "https" } else { "http" };
    let user = read_string_field(config, "user", "default");
    let password = read_string_field(config, "password", "");
    let database = database_override
        .map(ToString::to_string)
        .unwrap_or_else(|| read_string_field(config, "database", ""));
    let mut url = format!("{scheme}://{host}:{port}/?default_format=JSON");
    if !database.is_empty() {
        url.push_str("&database=");
        url.push_str(&url_encode(&database));
    }
    let sql_with_format = clickhouse_query_with_json_format(sql);
    let mut posix = format!(
        "curl -fsS -u {} --data-binary {} {}",
        shell_quote(&format!("{user}:{password}")),
        shell_quote(&sql_with_format),
        shell_quote(&url)
    );
    let mut windows = format!(
        "curl.exe -fsS -u {} --data-binary {} {}",
        ps_quote(&format!("{user}:{password}")),
        ps_quote(&sql_with_format),
        ps_quote(&url)
    );
    if password.is_empty() {
        posix = format!(
            "curl -fsS --data-binary {} {}",
            shell_quote(&sql_with_format),
            shell_quote(&url)
        );
        windows = format!(
            "curl.exe -fsS --data-binary {} {}",
            ps_quote(&sql_with_format),
            ps_quote(&url)
        );
    }
    let output = run_cli_output(
        state,
        connection_id,
        posix,
        Some(windows),
        "ClickHouse 查询失败。",
    )
    .await?;
    Ok(parse_clickhouse_response(&output))
}

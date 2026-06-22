use serde_json::{json, Value};

use super::{
    codec::encode_config_id,
    parse::{parse_csv_objects, parse_csv_query, parse_postgres_command_tag_row_count},
    session::{decode_active_db_session_args, register_db_session},
    should_fallback_to_database_cli, should_try_database_tunnel,
    sql::pg_literal,
    tunnel,
};
use crate::{ps_quote, read_string_field, run_cli_output, shell_quote, string_arg, AppState};

pub(crate) async fn postgres_connect(
    state: &AppState,
    window: &tauri::Window,
    args: Vec<Value>,
) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let config = args.get(1).cloned().unwrap_or_else(|| json!({}));
    let mut fallback_reason = None;
    if should_try_database_tunnel(state, &connection_id, &config)? {
        match tunnel::postgres_connect(state, window, args.clone()).await {
            Ok(result) => return Ok(result),
            Err(error) if should_fallback_to_database_cli(&config) => {
                eprintln!(
                    "[database] PostgreSQL SSH tunnel unavailable, falling back to CLI: {error}"
                );
                fallback_reason = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    let postgres_id = encode_config_id("postgres", &config)?;
    let _ = run_postgres_cli(state, &connection_id, &config, "SELECT 1 AS ok;").await?;
    register_db_session(state, "postgres", &connection_id, &postgres_id, config)?;
    Ok(json!({
        "postgresId": postgres_id,
        "transport": "ssh-exec",
        "fallbackReason": fallback_reason,
    }))
}

pub(crate) async fn postgres_databases(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    if tunnel::has_session(state, "postgres", &args)? {
        return tunnel::postgres_databases(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "postgres", &args, 0, 1)?;
    let output = run_postgres_cli(
        state,
        &connection_id,
        &config,
        "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname;",
    )
    .await?;
    let rows = parse_csv_objects(&output)?;
    Ok(json!(rows
        .into_iter()
        .filter_map(|row| row.get("datname").cloned())
        .collect::<Vec<_>>()))
}

pub(crate) async fn postgres_schemas(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    if tunnel::has_session(state, "postgres", &args)? {
        return tunnel::postgres_schemas(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "postgres", &args, 0, 1)?;
    let output = run_postgres_cli(
        state,
        &connection_id,
        &config,
        "SELECT nspname AS schema_name \
         FROM pg_catalog.pg_namespace \
         WHERE nspname <> 'information_schema' AND nspname NOT LIKE 'pg_%' \
         ORDER BY nspname;",
    )
    .await?;
    let rows = parse_csv_objects(&output)?;
    Ok(json!(rows
        .into_iter()
        .filter_map(|row| row.get("schema_name").cloned())
        .collect::<Vec<_>>()))
}

pub(crate) async fn postgres_tables(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    if tunnel::has_session(state, "postgres", &args)? {
        return tunnel::postgres_tables(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "postgres", &args, 0, 1)?;
    let schema = string_arg(&args, 2)?;
    let sql = format!(
        "SELECT n.nspname AS table_schema, \
                c.relname AS table_name, \
                CASE c.relkind \
                  WHEN 'r' THEN 'BASE TABLE' \
                  WHEN 'p' THEN 'PARTITIONED TABLE' \
                  WHEN 'v' THEN 'VIEW' \
                  WHEN 'm' THEN 'MATERIALIZED VIEW' \
                  WHEN 'f' THEN 'FOREIGN TABLE' \
                  ELSE c.relkind::text \
                END AS table_type \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {} AND c.relkind IN ('r', 'p', 'v', 'm', 'f') \
         ORDER BY c.relname;",
        pg_literal(&schema)
    );
    let output = run_postgres_cli(state, &connection_id, &config, &sql).await?;
    let rows = parse_csv_objects(&output)?;
    Ok(json!(rows
        .into_iter()
        .map(|row| json!({
            "schema": row.get("table_schema").cloned().unwrap_or_default(),
            "name": row.get("table_name").cloned().unwrap_or_default(),
            "type": row.get("table_type").cloned().unwrap_or_default()
        }))
        .collect::<Vec<_>>()))
}

pub(crate) async fn postgres_columns(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    if tunnel::has_session(state, "postgres", &args)? {
        return tunnel::postgres_columns(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "postgres", &args, 0, 1)?;
    let schema = string_arg(&args, 2)?;
    let table = string_arg(&args, 3)?;
    let sql = format!(
        r#"
SELECT
  c.column_name,
  c.data_type,
  c.is_nullable,
  c.column_default,
  EXISTS (
    SELECT 1
    FROM information_schema.table_constraints tc
    JOIN information_schema.key_column_usage kcu
      ON tc.constraint_name = kcu.constraint_name
      AND tc.table_schema = kcu.table_schema
      AND tc.table_name = kcu.table_name
    WHERE tc.constraint_type = 'PRIMARY KEY'
      AND tc.table_schema = c.table_schema
      AND tc.table_name = c.table_name
      AND kcu.column_name = c.column_name
  ) AS is_primary_key
FROM information_schema.columns c
WHERE c.table_schema = {} AND c.table_name = {}
ORDER BY c.ordinal_position;
"#,
        pg_literal(&schema),
        pg_literal(&table)
    );
    let output = run_postgres_cli(state, &connection_id, &config, &sql).await?;
    let rows = parse_csv_objects(&output)?;
    Ok(json!(rows
        .into_iter()
        .map(|row| json!({
            "name": row.get("column_name").cloned().unwrap_or_default(),
            "dataType": row.get("data_type").cloned().unwrap_or_default(),
            "nullable": row.get("is_nullable").is_some_and(|value| value == "YES"),
            "defaultValue": row.get("column_default").cloned(),
            "isPrimaryKey": row.get("is_primary_key").is_some_and(|value| value == "t" || value == "true")
        }))
        .collect::<Vec<_>>()))
}

pub(crate) async fn postgres_query(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    if tunnel::has_session(state, "postgres", &args)? {
        return tunnel::postgres_query(state, args).await;
    }
    let (connection_id, config) = decode_active_db_session_args(state, "postgres", &args, 0, 1)?;
    let sql = string_arg(&args, 2)?;
    let output = run_postgres_cli(state, &connection_id, &config, &sql).await?;
    if let Some(row_count) = parse_postgres_command_tag_row_count(&output) {
        let mut result = json!({ "columns": [], "rows": [] });
        if let Some(row_count) = row_count {
            result["rowCount"] = json!(row_count);
        }
        return Ok(result);
    }
    let (columns, rows) = parse_csv_query(&output)?;
    Ok(json!({ "columns": columns, "rows": rows, "rowCount": rows.len() }))
}

async fn run_postgres_cli(
    state: &AppState,
    connection_id: &str,
    config: &Value,
    sql: &str,
) -> Result<String, String> {
    let host = read_string_field(config, "host", "127.0.0.1");
    let port = config.get("port").and_then(Value::as_u64).unwrap_or(5432);
    let user = read_string_field(config, "user", "postgres");
    let password = read_string_field(config, "password", "");
    let database = read_string_field(config, "database", "postgres");
    let posix = format!(
        "PGPASSWORD={} psql --no-psqlrc --csv -h {} -p {} -U {} -d {} -c {}",
        shell_quote(&password),
        shell_quote(&host),
        port,
        shell_quote(&user),
        shell_quote(&database),
        shell_quote(sql)
    );
    let windows = format!(
        "$env:PGPASSWORD = {}; psql --no-psqlrc --csv -h {} -p {} -U {} -d {} -c {}",
        ps_quote(&password),
        ps_quote(&host),
        port,
        ps_quote(&user),
        ps_quote(&database),
        ps_quote(sql)
    );
    run_cli_output(
        state,
        connection_id,
        posix,
        Some(windows),
        "PostgreSQL 命令执行失败。",
    )
    .await
}

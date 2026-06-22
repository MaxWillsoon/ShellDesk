use serde::{Deserialize, Serialize};

use super::TunnelOptions;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MysqlConnectConfig {
    #[serde(default = "default_mode")]
    pub(super) mode: String,
    #[serde(default = "default_host")]
    pub(super) host: String,
    #[serde(default = "default_mysql_port")]
    pub(super) port: u16,
    #[serde(default = "default_mysql_user")]
    pub(super) user: String,
    #[serde(default)]
    pub(super) password: String,
    #[serde(default)]
    pub(super) database: Option<String>,
    #[serde(default)]
    pub(super) tunnel: Option<TunnelOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PostgresConnectConfig {
    #[serde(default = "default_mode")]
    pub(super) mode: String,
    #[serde(default = "default_host")]
    pub(super) host: String,
    #[serde(default = "default_pg_port")]
    pub(super) port: u16,
    #[serde(default = "default_pg_user")]
    pub(super) user: String,
    #[serde(default)]
    pub(super) password: String,
    #[serde(default = "default_pg_database")]
    pub(super) database: String,
    #[serde(default)]
    pub(super) tunnel: Option<TunnelOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RedisConnectConfig {
    #[serde(default = "default_mode")]
    pub(super) mode: String,
    #[serde(default = "default_host")]
    pub(super) host: String,
    #[serde(default = "default_redis_port")]
    pub(super) port: u16,
    #[serde(default)]
    pub(super) password: String,
    #[serde(default, alias = "db")]
    pub(super) database: u8,
    #[serde(default)]
    pub(super) tunnel: Option<TunnelOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ClickHouseConnectConfig {
    #[serde(default = "default_mode")]
    pub(super) mode: String,
    #[serde(default = "default_host")]
    pub(super) host: String,
    #[serde(default)]
    pub(super) port: Option<u16>,
    #[serde(default = "default_clickhouse_user")]
    pub(super) user: String,
    #[serde(default)]
    pub(super) password: String,
    #[serde(default)]
    pub(super) database: String,
    #[serde(default)]
    pub(super) secure: bool,
    #[serde(default)]
    pub(super) tunnel: Option<TunnelOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MongoConnectConfig {
    #[serde(default = "default_mode")]
    pub(super) mode: String,
    #[serde(default = "default_host")]
    pub(super) host: String,
    #[serde(default = "default_mongo_port")]
    pub(super) port: u16,
    #[serde(default)]
    pub(super) username: String,
    #[serde(default)]
    pub(super) password: String,
    #[serde(default = "default_mongo_auth_source")]
    pub(super) auth_source: String,
    #[serde(default)]
    pub(super) tunnel: Option<TunnelOptions>,
}

pub(super) fn clickhouse_port(config: &ClickHouseConnectConfig) -> u16 {
    config
        .port
        .unwrap_or(if config.secure { 8443 } else { 8123 })
}

fn default_mode() -> String {
    "auto".to_string()
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_mysql_port() -> u16 {
    3306
}

fn default_mysql_user() -> String {
    "root".to_string()
}

fn default_pg_port() -> u16 {
    5432
}

fn default_pg_user() -> String {
    "postgres".to_string()
}

fn default_pg_database() -> String {
    "postgres".to_string()
}

fn default_redis_port() -> u16 {
    6379
}

fn default_clickhouse_user() -> String {
    "default".to_string()
}

fn default_mongo_port() -> u16 {
    27017
}

fn default_mongo_auth_source() -> String {
    "admin".to_string()
}

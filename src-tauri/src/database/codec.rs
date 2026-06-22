use base64::Engine;
use serde_json::Value;

use crate::error_string;

pub(super) fn encode_config_id(prefix: &str, value: &Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(error_string)?;
    Ok(format!(
        "{}:{}",
        prefix,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    ))
}

pub(super) fn url_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

pub(super) fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

pub(super) fn mongo_ejson_prelude() -> &'static str {
    "const __shelldeskParseEjson = (text) => { if (typeof EJSON !== 'undefined' && EJSON.parse) { return EJSON.parse(text, { relaxed: true }); } return JSON.parse(text); }; const __shelldeskStringify = (value) => { if (typeof EJSON !== 'undefined' && EJSON.stringify) { try { return EJSON.stringify(value, null, 0, { relaxed: false }); } catch (_error) { return EJSON.stringify(value, { relaxed: false }); } } return JSON.stringify(value); };"
}

pub(super) fn mongo_ejson_value_expression(raw: &str, fallback_js: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        fallback_js.to_string()
    } else {
        format!("__shelldeskParseEjson({})", js_string(trimmed))
    }
}

pub(super) fn json_to_cli_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

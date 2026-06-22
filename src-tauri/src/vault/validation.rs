use crate::error_string;
use serde_json::Value;

pub(super) fn read_bounded_string_value(
    value: Option<&Value>,
    label: &str,
    max_length: usize,
    required: bool,
    trim: bool,
    reject_line_breaks: bool,
) -> Result<String, String> {
    let Some(Value::String(value)) = value else {
        return Err(format!("{label}无效。"));
    };
    read_bounded_string(value, label, max_length, required, trim, reject_line_breaks)
}

pub(super) fn read_bounded_string(
    value: &str,
    label: &str,
    max_length: usize,
    required: bool,
    trim: bool,
    reject_line_breaks: bool,
) -> Result<String, String> {
    let next_value = if trim {
        value.trim().to_string()
    } else {
        value.to_string()
    };
    if required && next_value.is_empty() {
        return Err(format!("请输入{}。", label));
    }
    if next_value.chars().count() > max_length
        || next_value.contains('\0')
        || (reject_line_breaks && next_value.contains(['\r', '\n']))
    {
        return Err(format!("{}无效。", label));
    }
    Ok(next_value)
}

pub(super) fn clone_json_with_size_limit(
    value: Value,
    max_bytes: usize,
    message: &str,
) -> Result<Value, String> {
    let serialized = serde_json::to_vec(&value).map_err(error_string)?;
    if serialized.len() > max_bytes {
        return Err(message.to_string());
    }
    serde_json::from_slice(&serialized).map_err(|_| message.to_string())
}

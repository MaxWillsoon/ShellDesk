use super::{clone_json_with_size_limit, read_bounded_string};
use serde_json::{json, Value};

pub(crate) fn get_preference(store: &Value, raw_key: &str) -> Result<Value, String> {
    let key = read_preference_key(raw_key)?;
    Ok(store
        .get("preferences")
        .and_then(|preferences| preferences.get(&key))
        .cloned()
        .unwrap_or(Value::Null))
}

pub(crate) fn set_preference_to_store(
    store: &mut Value,
    raw_key: &str,
    raw_value: Value,
) -> Result<Value, String> {
    let key = read_preference_key(raw_key)?;
    let value = read_preference_value(raw_value)?;
    let Some(store_object) = store.as_object_mut() else {
        return Err("本地数据无效。".to_string());
    };
    if !store_object
        .get("preferences")
        .is_some_and(Value::is_object)
    {
        store_object.insert("preferences".to_string(), json!({}));
    }
    let preferences = store_object
        .get_mut("preferences")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "本地数据无效。".to_string())?;
    if value.is_null() {
        preferences.remove(&key);
    } else {
        preferences.insert(key.clone(), value.clone());
    }
    Ok(preferences.get(&key).cloned().unwrap_or(Value::Null))
}

fn read_preference_key(value: &str) -> Result<String, String> {
    let key = read_bounded_string(value, "偏好设置键", 255, true, true, true)?;
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '.' | '_' | '%' | '-'))
    {
        return Err("偏好设置键无效。".to_string());
    }
    Ok(key)
}

fn read_preference_value(value: Value) -> Result<Value, String> {
    clone_json_with_size_limit(value, 64 * 1024, "偏好设置内容无效或超过大小限制。")
}

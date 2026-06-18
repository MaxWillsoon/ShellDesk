use crate::{error_string, random_id, string_arg, value_to_bytes, AppState};
use chrono::Utc;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

const MAX_ZMODEM_READ_CHUNK_BYTES: u64 = 256 * 1024;
const MAX_ZMODEM_UPLOAD_SELECTION_AGE_MS: i64 = 30 * 60 * 1000;

#[derive(Clone)]
pub(crate) struct ZmodemUploadSelection {
    path: PathBuf,
    size: u64,
    expires_at: i64,
}

pub(crate) fn select_zmodem_upload_files(state: &AppState) -> Result<Value, String> {
    cleanup_expired_zmodem_upload_selections(state)?;
    let Some(paths) = rfd::FileDialog::new()
        .set_title("选择要通过 ZMODEM 上传的文件")
        .add_filter("所有文件", &["*"])
        .pick_files()
    else {
        return Ok(json!({ "canceled": true, "files": [] }));
    };
    let mut files = Vec::new();
    let mut selections = state
        .zmodem_upload_selections
        .lock()
        .map_err(error_string)?;
    for path in paths {
        let metadata = fs::metadata(&path).map_err(error_string)?;
        if !metadata.is_file() {
            continue;
        }
        let id = random_id("zmodem");
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "upload".to_string());
        let size = metadata.len();
        let last_modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        selections.insert(
            id.clone(),
            ZmodemUploadSelection {
                path,
                size,
                expires_at: zmodem_expires_at(),
            },
        );
        files.push(json!({
            "id": id,
            "name": name,
            "size": size,
            "lastModified": last_modified
        }));
    }
    if files.is_empty() {
        return Err("没有可上传的本地文件。".to_string());
    }
    Ok(json!({ "canceled": false, "files": files }))
}

pub(crate) fn read_zmodem_upload_file(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    let file_id = string_arg(&args, 0)?;
    let (offset, length) = read_zmodem_range(&args)?;
    let selection = get_zmodem_upload_selection(state, &file_id)?;
    if offset >= selection.size {
        return Ok(json!([]));
    }
    let read_len = length.min(selection.size.saturating_sub(offset)) as usize;
    let mut file = fs::File::open(&selection.path).map_err(error_string)?;
    file.seek(SeekFrom::Start(offset)).map_err(error_string)?;
    let mut buffer = vec![0_u8; read_len];
    let bytes_read = file.read(&mut buffer).map_err(error_string)?;
    buffer.truncate(bytes_read);
    Ok(json!(buffer))
}

pub(crate) fn release_zmodem_upload_files(
    state: &AppState,
    args: Vec<Value>,
) -> Result<Value, String> {
    let ids = args
        .first()
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut selections = state
        .zmodem_upload_selections
        .lock()
        .map_err(error_string)?;
    for id in ids {
        if let Some(id) = id.as_str() {
            selections.remove(id);
        }
    }
    cleanup_expired_zmodem_upload_selections_locked(&mut selections);
    Ok(json!(true))
}

fn get_zmodem_upload_selection(
    state: &AppState,
    id: &str,
) -> Result<ZmodemUploadSelection, String> {
    let mut selections = state
        .zmodem_upload_selections
        .lock()
        .map_err(error_string)?;
    cleanup_expired_zmodem_upload_selections_locked(&mut selections);
    let selection = selections
        .get_mut(id)
        .ok_or_else(|| "上传文件选择已过期，请重新选择文件。".to_string())?;
    selection.expires_at = zmodem_expires_at();
    Ok(selection.clone())
}

fn cleanup_expired_zmodem_upload_selections(state: &AppState) -> Result<(), String> {
    let mut selections = state
        .zmodem_upload_selections
        .lock()
        .map_err(error_string)?;
    cleanup_expired_zmodem_upload_selections_locked(&mut selections);
    Ok(())
}

fn cleanup_expired_zmodem_upload_selections_locked(
    selections: &mut HashMap<String, ZmodemUploadSelection>,
) {
    let now = current_time_millis();
    selections.retain(|_, selection| selection.expires_at > now);
}

fn zmodem_expires_at() -> i64 {
    current_time_millis().saturating_add(MAX_ZMODEM_UPLOAD_SELECTION_AGE_MS)
}

fn current_time_millis() -> i64 {
    Utc::now().timestamp_millis()
}

fn read_zmodem_range(args: &[Value]) -> Result<(u64, u64), String> {
    let offset = args
        .get(1)
        .and_then(Value::as_u64)
        .ok_or_else(invalid_zmodem_range_error)?;
    let length = args
        .get(2)
        .and_then(Value::as_u64)
        .ok_or_else(invalid_zmodem_range_error)?;
    if length == 0 || length > MAX_ZMODEM_READ_CHUNK_BYTES {
        return Err(invalid_zmodem_range_error());
    }
    Ok((offset, length))
}

fn invalid_zmodem_range_error() -> String {
    "读取上传文件的范围无效。".to_string()
}

fn sanitize_zmodem_file_name(file_name: &str) -> String {
    let safe_name = file_name
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>()
        .trim_end_matches(['.', ' '])
        .trim()
        .to_string();
    if safe_name.is_empty() || is_windows_reserved_file_name(&safe_name) {
        "download".to_string()
    } else {
        safe_name
    }
}

fn is_windows_reserved_file_name(file_name: &str) -> bool {
    let stem = file_name
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        stem.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

pub(crate) fn save_zmodem_file(args: Vec<Value>) -> Result<Value, String> {
    let file_name = sanitize_zmodem_file_name(args.first().and_then(Value::as_str).unwrap_or(""));
    let content = value_to_bytes(args.get(1).cloned().unwrap_or(Value::Null))?;
    let Some(path) = rfd::FileDialog::new()
        .set_file_name(&file_name)
        .add_filter("所有文件", &["*"])
        .save_file()
    else {
        return Ok(json!({ "canceled": true }));
    };
    fs::write(&path, &content).map_err(error_string)?;
    Ok(json!({
        "canceled": false,
        "filePath": path.to_string_lossy(),
        "size": content.len()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zmodem_range_rejects_invalid_offsets_and_lengths() {
        assert_eq!(
            read_zmodem_range(&[json!("file-1"), json!(-1), json!(1024)]).unwrap_err(),
            "读取上传文件的范围无效。"
        );
        assert_eq!(
            read_zmodem_range(&[json!("file-1"), json!(0), json!(0)]).unwrap_err(),
            "读取上传文件的范围无效。"
        );
        assert_eq!(
            read_zmodem_range(&[
                json!("file-1"),
                json!(0),
                json!(MAX_ZMODEM_READ_CHUNK_BYTES + 1)
            ])
            .unwrap_err(),
            "读取上传文件的范围无效。"
        );
        assert_eq!(
            read_zmodem_range(&[json!("file-1"), json!(4), json!(1024)]).unwrap(),
            (4, 1024)
        );
    }

    #[test]
    fn zmodem_download_file_name_matches_legacy_sanitization() {
        assert_eq!(
            sanitize_zmodem_file_name("logs/../prod:dump?.txt "),
            "logs_.._prod_dump_.txt"
        );
        assert_eq!(sanitize_zmodem_file_name("CON.txt"), "download");
        assert_eq!(sanitize_zmodem_file_name("..."), "download");
        assert_eq!(sanitize_zmodem_file_name(" report.md "), "report.md");
    }
}

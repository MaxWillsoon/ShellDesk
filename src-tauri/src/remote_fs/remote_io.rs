use super::commands::{
    remote_directory_archive_command, remote_file_read_command, remote_file_write_command,
};
use super::local_transfer::create_tar_gz_archive;
use crate::{
    get_connection, random_id, run_connection_command_with_options,
    run_ssh_command_for_profile_interactive, shell_quote, AppState, SshProfile,
};
use base64::Engine;
use serde_json::{json, Value};
use std::path::Path;

pub(super) async fn remote_path_kind(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
) -> Result<String, String> {
    let command = format!(
        "if [ -d {path} ]; then echo directory; elif [ -f {path} ]; then echo file; elif [ -L {path} ]; then echo symlink; else echo missing; fi",
        path = shell_quote(remote_path)
    );
    let output =
        run_ssh_command_for_profile_interactive(state.clone(), profile, command, String::new())
            .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("检查远程路径失败。")
            .to_string());
    }
    Ok(output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string())
}

pub(super) async fn remote_path_size(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
) -> Result<u64, String> {
    let command = format!(
        "if [ -d {path} ]; then du -sb {path} 2>/dev/null | awk '{{print $1}}'; else stat -c %s {path} 2>/dev/null || wc -c < {path}; fi",
        path = shell_quote(remote_path)
    );
    let output =
        run_ssh_command_for_profile_interactive(state.clone(), profile, command, String::new())
            .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Ok(0);
    }
    Ok(output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("0")
        .trim()
        .parse::<u64>()
        .unwrap_or(0))
}

async fn download_remote_directory_archive(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
) -> Result<Vec<u8>, String> {
    let command = remote_directory_archive_command(remote_path);
    let output =
        run_ssh_command_for_profile_interactive(state.clone(), profile, command, String::new())
            .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("打包远程目录失败。")
            .to_string());
    }
    let encoded = output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("远程目录归档解码失败：{error}"))
}

pub(super) async fn download_remote_directory_archive_with_options(
    state: &AppState,
    connection_id: &str,
    profile: SshProfile,
    remote_path: &str,
    options: Option<&Value>,
) -> Result<Vec<u8>, String> {
    match download_remote_directory_archive(state, profile, remote_path).await {
        Ok(bytes) => Ok(bytes),
        Err(first_error) => {
            if !can_retry_remote_file_with_privilege(state, connection_id, options)? {
                return Err(first_error);
            }
            let encoded = run_privileged_remote_output_base64(
                state,
                connection_id,
                remote_directory_archive_command(remote_path),
                options,
                &first_error,
            )
            .await?;
            base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|error| format!("远程目录归档解码失败：{error}"))
        }
    }
}

pub(super) async fn upload_local_directory_to_remote(
    state: &AppState,
    profile: SshProfile,
    local_dir: &Path,
    remote_dir: &str,
) -> Result<(), String> {
    let archive = create_tar_gz_archive(local_dir).await?;
    let remote_archive = format!("/tmp/{}.tar.gz", random_id("shelldesk-upload"));
    write_remote_file_bytes(state, profile.clone(), &remote_archive, &archive).await?;
    let command = format!(
        "mkdir -p -- {remote_dir} && tar -xzf {archive} -C {remote_dir}; code=$?; rm -f -- {archive}; exit $code",
        remote_dir = shell_quote(remote_dir),
        archive = shell_quote(&remote_archive)
    );
    let output =
        run_ssh_command_for_profile_interactive(state.clone(), profile, command, String::new())
            .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("解包上传目录失败。")
            .to_string());
    }
    Ok(())
}

async fn read_remote_file_bytes(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
) -> Result<Vec<u8>, String> {
    let command = remote_file_read_command(remote_path);
    let output =
        run_ssh_command_for_profile_interactive(state.clone(), profile, command, String::new())
            .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("下载远程文件失败。")
            .to_string());
    }
    let encoded = output
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("远程文件 base64 解码失败：{error}"))
}

pub(super) async fn read_remote_file_bytes_with_options(
    state: &AppState,
    connection_id: &str,
    profile: SshProfile,
    remote_path: &str,
    options: Option<&Value>,
) -> Result<Vec<u8>, String> {
    match read_remote_file_bytes(state, profile, remote_path).await {
        Ok(bytes) => Ok(bytes),
        Err(first_error) => {
            if !can_retry_remote_file_with_privilege(state, connection_id, options)? {
                return Err(first_error);
            }
            let encoded = run_privileged_remote_output_base64(
                state,
                connection_id,
                remote_file_read_command(remote_path),
                options,
                &first_error,
            )
            .await?;
            base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|error| format!("远程文件 base64 解码失败：{error}"))
        }
    }
}

async fn write_remote_file_bytes(
    state: &AppState,
    profile: SshProfile,
    remote_path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let command = remote_file_write_command(remote_path);
    let output =
        run_ssh_command_for_profile_interactive(state.clone(), profile, command, encoded).await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) != 0 {
        return Err(output
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("上传文件失败。")
            .to_string());
    }
    Ok(())
}

pub(super) async fn write_remote_file_bytes_with_options(
    state: &AppState,
    connection_id: &str,
    profile: SshProfile,
    remote_path: &str,
    bytes: &[u8],
    options: Option<&Value>,
) -> Result<(), String> {
    match write_remote_file_bytes(state, profile, remote_path, bytes).await {
        Ok(()) => Ok(()),
        Err(first_error) => {
            if !can_retry_remote_file_with_privilege(state, connection_id, options)? {
                return Err(first_error);
            }

            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            let output = run_connection_command_with_options(
                state.clone(),
                vec![
                    json!(connection_id),
                    json!(remote_file_write_command(remote_path)),
                    json!(encoded),
                    options.cloned().unwrap_or(Value::Null),
                ],
                3,
            )
            .await?;
            if output.get("code").and_then(Value::as_i64).unwrap_or(1) == 0 {
                Ok(())
            } else {
                Err(output
                    .get("stderr")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(&first_error)
                    .to_string())
            }
        }
    }
}

async fn run_privileged_remote_output_base64(
    state: &AppState,
    connection_id: &str,
    command: String,
    options: Option<&Value>,
    fallback_error: &str,
) -> Result<String, String> {
    let output = run_connection_command_with_options(
        state.clone(),
        vec![
            json!(connection_id),
            json!(command),
            json!(""),
            options.cloned().unwrap_or(Value::Null),
        ],
        3,
    )
    .await?;
    if output.get("code").and_then(Value::as_i64).unwrap_or(1) == 0 {
        return Ok(output
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string());
    }
    Err(output
        .get("stderr")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_error)
        .to_string())
}

pub(super) fn can_retry_remote_file_with_privilege(
    state: &AppState,
    connection_id: &str,
    options: Option<&Value>,
) -> Result<bool, String> {
    if options
        .and_then(Value::as_object)
        .is_some_and(|object| object.contains_key("sudoPassword"))
    {
        return Ok(true);
    }
    Ok(get_connection(state, connection_id)?.privilege.is_some())
}

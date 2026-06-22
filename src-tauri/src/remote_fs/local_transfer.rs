use super::transfer::TransferReporter;
use crate::{error_string, prevent_process_window, prevent_tokio_process_window, random_id};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::Command as StdCommand,
};
use tokio::process::Command;

const TRANSFER_COPY_CHUNK_BYTES: usize = 256 * 1024;

pub(super) fn local_path_file_stats(path: &Path) -> (u64, u64) {
    let Ok(metadata) = fs::metadata(path) else {
        return (0, 0);
    };
    if metadata.is_file() {
        return (metadata.len(), 1);
    }
    if !metadata.is_dir() {
        return (0, 0);
    }
    let mut total_size = 0_u64;
    let mut file_count = 0_u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let (size, files) = local_path_file_stats(&entry.path());
            total_size = total_size.saturating_add(size);
            file_count = file_count.saturating_add(files);
        }
    }
    (total_size, file_count)
}

pub(super) fn copy_local_path_with_transfer(
    transfer: &TransferReporter,
    source: &Path,
    target: &Path,
) -> Result<(u64, u64), String> {
    transfer.check_canceled()?;
    let metadata = fs::metadata(source).map_err(error_string)?;
    if metadata.is_file() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(error_string)?;
        }
        let file_name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "transfer".to_string());
        transfer.start_file(&file_name, metadata.len());
        let copied = copy_local_file_with_transfer(transfer, source, target)?;
        transfer.complete_file();
        return Ok((copied, 1));
    }
    if !metadata.is_dir() {
        return Ok((0, 0));
    }
    fs::create_dir_all(target).map_err(error_string)?;
    let mut total_size = 0_u64;
    let mut file_count = 0_u64;
    for entry in fs::read_dir(source).map_err(error_string)? {
        let entry = entry.map_err(error_string)?;
        let child_source = entry.path();
        let child_target = target.join(entry.file_name());
        let (size, files) = copy_local_path_with_transfer(transfer, &child_source, &child_target)?;
        total_size = total_size.saturating_add(size);
        file_count = file_count.saturating_add(files);
    }
    Ok((total_size, file_count))
}

pub(super) fn copy_local_file_with_transfer(
    transfer: &TransferReporter,
    source: &Path,
    target: &Path,
) -> Result<u64, String> {
    transfer.check_canceled()?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(error_string)?;
    }
    let mut input = fs::File::open(source).map_err(error_string)?;
    let mut output = fs::File::create(target).map_err(error_string)?;
    let mut buffer = vec![0_u8; TRANSFER_COPY_CHUNK_BYTES];
    let mut copied = 0_u64;
    loop {
        transfer.check_canceled()?;
        let read = input.read(&mut buffer).map_err(error_string)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(error_string)?;
        copied = copied.saturating_add(read as u64);
        transfer.add_bytes(read as u64);
    }
    output.flush().map_err(error_string)?;
    Ok(copied)
}

pub(super) async fn create_tar_gz_archive(local_dir: &Path) -> Result<Vec<u8>, String> {
    let parent = local_dir
        .parent()
        .ok_or_else(|| "目录没有可用的父路径。".to_string())?;
    let name = local_dir
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| "目录名无效。".to_string())?;
    let archive_path =
        std::env::temp_dir().join(format!("{}.tar.gz", random_id("shelldesk-upload")));
    let mut command = Command::new("tar");
    prevent_tokio_process_window(&mut command);
    let output = command
        .arg("-czf")
        .arg(&archive_path)
        .arg("-C")
        .arg(parent)
        .arg(&name)
        .output()
        .await
        .map_err(error_string)?;
    if !output.status.success() {
        let _ = fs::remove_file(&archive_path);
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let bytes = fs::read(&archive_path).map_err(error_string)?;
    let _ = fs::remove_file(&archive_path);
    Ok(bytes)
}

pub(super) fn extract_tar_gz_archive(bytes: &[u8], target_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(target_dir).map_err(error_string)?;
    let archive_path =
        std::env::temp_dir().join(format!("{}.tar.gz", random_id("shelldesk-download")));
    fs::write(&archive_path, bytes).map_err(error_string)?;
    let mut command = StdCommand::new("tar");
    prevent_process_window(&mut command);
    let output = command
        .arg("-xzf")
        .arg(&archive_path)
        .arg("-C")
        .arg(target_dir)
        .output()
        .map_err(error_string)?;
    let _ = fs::remove_file(&archive_path);
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

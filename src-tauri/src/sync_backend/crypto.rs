use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use rand::Rng;
use serde_json::{json, Value};
use sha2::Sha256;

use crate::error_string;

fn derive_sync_key(passphrase: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, iterations, &mut key);
    key
}

pub(super) fn encrypt_remote_document(document: &Value, passphrase: &str) -> Result<Value, String> {
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    rand::thread_rng().fill(&mut salt);
    rand::thread_rng().fill(&mut iv);
    let key = derive_sync_key(passphrase, &salt, 210_000);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(error_string)?;
    let plaintext = serde_json::to_vec(document).map_err(error_string)?;
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&iv), plaintext.as_slice())
        .map_err(|_| "远端同步文件加密失败。".to_string())?;
    if encrypted.len() < 16 {
        return Err("远端同步文件加密失败。".to_string());
    }
    let (ciphertext, tag) = encrypted.split_at(encrypted.len() - 16);
    Ok(json!({
        "format": "shelldesk-sync-encrypted",
        "version": 1,
        "algorithm": "aes-256-gcm",
        "kdf": "pbkdf2-sha256",
        "iterations": 210000,
        "salt": base64::engine::general_purpose::STANDARD.encode(salt),
        "iv": base64::engine::general_purpose::STANDARD.encode(iv),
        "tag": base64::engine::general_purpose::STANDARD.encode(tag),
        "ciphertext": base64::engine::general_purpose::STANDARD.encode(ciphertext)
    }))
}

pub(super) fn decrypt_remote_document(wrapper: &Value, passphrase: &str) -> Result<Value, String> {
    if wrapper.get("format").and_then(Value::as_str) != Some("shelldesk-sync-encrypted") {
        return Err("远端同步文件不是 ShellDesk 加密同步包。".to_string());
    }
    let decode = |key: &str| -> Result<Vec<u8>, String> {
        base64::engine::general_purpose::STANDARD
            .decode(wrapper.get(key).and_then(Value::as_str).unwrap_or(""))
            .map_err(|_| "同步密码不正确，或远端同步文件已损坏。".to_string())
    };
    let salt = decode("salt")?;
    let iv = decode("iv")?;
    let tag = decode("tag")?;
    let mut ciphertext = decode("ciphertext")?;
    let iterations = wrapper
        .get("iterations")
        .and_then(Value::as_u64)
        .unwrap_or(210_000)
        .clamp(100_000, 1_000_000) as u32;
    ciphertext.extend(tag);
    let key = derive_sync_key(passphrase, &salt, iterations);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(error_string)?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&iv), ciphertext.as_slice())
        .map_err(|_| "同步密码不正确，或远端同步文件已损坏。".to_string())?;
    serde_json::from_slice(&plaintext).map_err(|_| "远端同步文件内容无效。".to_string())
}

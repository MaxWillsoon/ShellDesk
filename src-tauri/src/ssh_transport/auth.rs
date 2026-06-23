use crate::askpass::AskpassBroker;
use crate::proxy::{proxy_helper_env_name, SshProxyConfig};
use crate::{prevent_process_window, SshProfile};
use base64::Engine;
use portable_pty::CommandBuilder;
use serde_json::json;
use std::process::{Command as StdCommand, Stdio};
use tokio::process::Command;

pub(crate) fn ssh_args(profile: &SshProfile) -> Vec<String> {
    ssh_args_with_askpass(profile, false)
}

pub(crate) fn ssh_args_with_askpass(profile: &SshProfile, allow_askpass: bool) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        profile.port.to_string(),
        "-o".to_string(),
        "ConnectTimeout=15".to_string(),
    ];
    args.push("-o".to_string());
    args.push("StrictHostKeyChecking=yes".to_string());
    args.push("-o".to_string());
    args.push(format!(
        "UserKnownHostsFile={}",
        connection_known_hosts_path(profile)
    ));
    args.push("-o".to_string());
    args.push(format!("GlobalKnownHostsFile={}", null_known_hosts_path()));
    if profile.auth_method != "password" {
        args.push("-o".to_string());
        args.push("PreferredAuthentications=publickey".to_string());
        args.push("-o".to_string());
        args.push("PasswordAuthentication=no".to_string());
        args.push("-o".to_string());
        args.push("KbdInteractiveAuthentication=no".to_string());
        args.push("-o".to_string());
        args.push("ChallengeResponseAuthentication=no".to_string());
    }
    if profile.auth_method != "password" && !allow_askpass {
        args.push("-o".to_string());
        args.push("BatchMode=yes".to_string());
    }
    if !profile.key_path.is_empty() {
        args.push("-i".to_string());
        args.push(profile.key_path.clone());
    }
    if let Some(command) = proxy_command_for_profile(profile) {
        args.push("-o".to_string());
        args.push(format!("ProxyCommand={command}"));
    }
    args
}

fn connection_known_hosts_path(profile: &SshProfile) -> String {
    if profile.known_hosts_path.trim().is_empty() {
        null_known_hosts_path().to_string()
    } else {
        profile.known_hosts_path.clone()
    }
}

fn null_known_hosts_path() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

pub(crate) fn should_use_sshpass(profile: &SshProfile) -> bool {
    profile.auth_method == "password" && !profile.password.is_empty() && command_exists("sshpass")
}

pub(crate) fn unavailable_password_auth_error(profile: &SshProfile) -> Option<String> {
    if let Some(jump) = profile.jump.as_deref() {
        if jump.auth_method == "password" && !jump.password.is_empty() && !command_exists("sshpass")
        {
            return Some("跳板机密码登录需要先安装 sshpass，或改用 SSH key/agent；当前 askpass 只能可靠处理目标主机的非交互密码提示。".to_string());
        }
        if let Some(error) = unavailable_password_auth_error(jump) {
            return Some(error);
        }
    }
    None
}

pub(crate) fn command_exists(name: &str) -> bool {
    let checker = if cfg!(windows) { "where" } else { "sh" };
    let mut command = StdCommand::new(checker);
    prevent_process_window(&mut command);
    if cfg!(windows) {
        command.arg(name);
    } else {
        command.args(["-lc", &format!("command -v {}", shell_quote(name))]);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn ssh_destination(profile: &SshProfile) -> String {
    if profile.username.trim().is_empty() {
        profile.address.clone()
    } else {
        format!("{}@{}", profile.username, profile.address)
    }
}

pub(super) fn proxy_command_for_profile(profile: &SshProfile) -> Option<String> {
    if let Some(jump) = profile.jump.as_deref() {
        let mut parts = if should_use_sshpass(jump) {
            vec!["sshpass".to_string(), "-e".to_string(), "ssh".to_string()]
        } else {
            vec!["ssh".to_string()]
        };
        parts.extend(
            ssh_args(jump)
                .into_iter()
                .map(|arg| proxy_command_arg(&arg)),
        );
        parts.push("-W".to_string());
        parts.push("%h:%p".to_string());
        parts.push(proxy_command_arg(&ssh_destination(jump)));
        return Some(parts.join(" "));
    }

    let proxy = profile.proxy.as_ref()?;
    match proxy.proxy_type.as_str() {
        "command" => Some(
            proxy
                .command
                .replace("{host}", "%h")
                .replace("{port}", "%p"),
        ),
        "http" | "socks5" => Some(network_proxy_command(profile, proxy)),
        _ => None,
    }
}

fn network_proxy_command(profile: &SshProfile, proxy: &SshProxyConfig) -> String {
    format!(
        "{} --shelldesk-proxy-helper {} %h %p",
        proxy_command_arg(&profile.proxy_helper_exe),
        proxy_command_arg(&proxy.helper_id)
    )
}

pub(super) fn apply_askpass_env_tokio(
    command: &mut Command,
    profile: &SshProfile,
    askpass_broker: Option<&AskpassBroker>,
) {
    if !profile_can_use_askpass(profile) {
        return;
    }
    let secret = askpass_secret(profile);
    if secret.is_none() && askpass_broker.is_none() {
        return;
    }
    if profile.proxy_helper_exe.trim().is_empty() {
        return;
    }
    command.env("SSH_ASKPASS", &profile.proxy_helper_exe);
    command.env("SSH_ASKPASS_REQUIRE", "force");
    command.env("SHELLDESK_ASKPASS_HELPER", "1");
    if let Some(secret) = secret {
        command.env(
            "SHELLDESK_ASKPASS_PASSWORD",
            base64::engine::general_purpose::STANDARD.encode(secret.as_bytes()),
        );
    }
    if let Some(broker) = askpass_broker {
        command.env("SHELLDESK_ASKPASS_BROKER", &broker.address);
        command.env("SHELLDESK_ASKPASS_TOKEN", &broker.token);
    }
    if std::env::var("DISPLAY").is_err() {
        command.env("DISPLAY", "shelldesk");
    }
}

pub(crate) fn apply_askpass_env_pty(
    command: &mut CommandBuilder,
    profile: &SshProfile,
    askpass_broker: Option<&AskpassBroker>,
) {
    if !profile_can_use_askpass(profile) {
        return;
    }
    let secret = askpass_secret(profile);
    if secret.is_none() && askpass_broker.is_none() {
        return;
    }
    if profile.proxy_helper_exe.trim().is_empty() {
        return;
    }
    command.env("SSH_ASKPASS", &profile.proxy_helper_exe);
    command.env("SSH_ASKPASS_REQUIRE", "force");
    command.env("SHELLDESK_ASKPASS_HELPER", "1");
    if let Some(secret) = secret {
        command.env(
            "SHELLDESK_ASKPASS_PASSWORD",
            base64::engine::general_purpose::STANDARD.encode(secret.as_bytes()),
        );
    }
    if let Some(broker) = askpass_broker {
        command.env("SHELLDESK_ASKPASS_BROKER", &broker.address);
        command.env("SHELLDESK_ASKPASS_TOKEN", &broker.token);
    }
    if std::env::var("DISPLAY").is_err() {
        command.env("DISPLAY", "shelldesk");
    }
}

pub(super) fn profile_can_use_askpass(profile: &SshProfile) -> bool {
    profile.auth_method == "password" || profile.auth_method != "agent"
}

pub(super) fn askpass_secret(profile: &SshProfile) -> Option<&str> {
    (!profile.password.is_empty()).then_some(profile.password.as_str())
}

pub(super) fn apply_proxy_helper_env_tokio(command: &mut Command, profile: &SshProfile) {
    for (name, value) in proxy_helper_envs(profile) {
        command.env(name, value);
    }
    if let Some(password) = jump_sshpass_password(profile) {
        command.env("SSHPASS", password);
    }
}

pub(crate) fn apply_proxy_helper_env_pty(command: &mut CommandBuilder, profile: &SshProfile) {
    for (name, value) in proxy_helper_envs(profile) {
        command.env(name, value);
    }
    if let Some(password) = jump_sshpass_password(profile) {
        command.env("SSHPASS", password);
    }
}

fn proxy_helper_envs(profile: &SshProfile) -> Vec<(String, String)> {
    let mut envs = Vec::new();
    collect_proxy_helper_envs(profile, &mut envs);
    envs
}

fn collect_proxy_helper_envs(profile: &SshProfile, envs: &mut Vec<(String, String)>) {
    if let Some(proxy) = profile.proxy.as_ref() {
        if matches!(proxy.proxy_type.as_str(), "http" | "socks5") {
            let config = json!({
                "type": proxy.proxy_type,
                "host": proxy.host,
                "port": proxy.port,
                "username": proxy.username,
                "password": proxy.password
            });
            if let Ok(bytes) = serde_json::to_vec(&config) {
                envs.push((
                    proxy_helper_env_name(&proxy.helper_id),
                    base64::engine::general_purpose::STANDARD.encode(bytes),
                ));
            }
        }
    }
    if let Some(jump) = profile.jump.as_deref() {
        collect_proxy_helper_envs(jump, envs);
    }
}

fn jump_sshpass_password(profile: &SshProfile) -> Option<&str> {
    let jump = profile.jump.as_deref()?;
    if should_use_sshpass(jump) {
        Some(jump.password.as_str())
    } else {
        jump_sshpass_password(jump)
    }
}

fn proxy_command_arg(value: &str) -> String {
    proxy_command_arg_for_platform(value, cfg!(windows))
}

pub(super) fn proxy_command_arg_for_platform(value: &str, windows: bool) -> String {
    if windows {
        if value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '.' | '_' | '-' | '/' | '\\' | ':' | '@' | '%' | '=')
        }) {
            value.to_string()
        } else {
            format!("\"{}\"", value.replace('"', "\\\""))
        }
    } else {
        shell_arg(value)
    }
}

fn shell_arg(value: &str) -> String {
    if value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/' | ':' | '@' | '%' | '=')
    }) {
        value.to_string()
    } else {
        shell_quote(value)
    }
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

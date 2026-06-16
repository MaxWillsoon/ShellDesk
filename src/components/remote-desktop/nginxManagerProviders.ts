import { powershellStdinCommand, type RemoteCommandInput } from './remoteSystem';
import { tCurrent } from '../../i18n';
import { shellSingleQuote } from './certManagerProviders';
import type { NginxDistro, NginxInstallation, NginxSitesLayout } from './nginxManagerTypes';

const nginxFieldMarker = '__SHELLDESK_NGINX_FIELD__';
const nginxFileMarker = '__SHELLDESK_NGINX_FILE__';

function windowsUnsupported(marker = nginxFieldMarker): RemoteCommandInput {
  return powershellStdinCommand(`[Console]::Out.WriteLine("${marker}|error|${tCurrent('auto.certManagerProviders.windowsUnsupported')}")`);
}

function sudoCommand(command: string) {
  return `if [ "$(id -u 2>/dev/null)" = "0" ]; then sh -c ${shellSingleQuote(command)}; else sudo -n sh -c ${shellSingleQuote(command)}; fi`;
}

function basename(filePath: string) {
  return filePath.split(/[\\/]/).pop() || filePath;
}

function normalizeNullable(value: string) {
  const trimmed = value.trim();
  return trimmed && trimmed !== 'null' ? trimmed : null;
}

function first(values: Map<string, string>, key: string, fallback = '') {
  return values.get(key)?.trim() || fallback;
}

function parseDistro(value: string): NginxDistro {
  if (value === 'debian' || value === 'rhel' || value === 'alpine') return value;
  return 'unknown';
}

function parseSitesLayout(value: string): NginxSitesLayout {
  return value === 'debian' ? 'debian' : 'rhel';
}

function createHeredocDelimiter(content: string) {
  let delimiter = 'SHELLDESK_NGINX_CONFIG_EOF';
  let index = 0;
  while (content.includes(delimiter)) {
    index += 1;
    delimiter = `SHELLDESK_NGINX_CONFIG_EOF_${index}`;
  }
  return delimiter;
}

export function createNginxDetectCommand(isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  return {
    command: `
emit() { printf '${nginxFieldMarker}|%s|%s\\n' "$1" "$2"; }
if ! command -v nginx >/dev/null 2>&1; then
  emit found false
  exit 0
fi
bin="$(command -v nginx)"
version="$(nginx -v 2>&1 | sed 's/^nginx version: //')"
build="$(nginx -V 2>&1 || true)"
config_path="$(printf '%s\\n' "$build" | sed -n 's/.*--conf-path=\\([^[:space:]]*\\).*/\\1/p' | head -n 1)"
error_log="$(printf '%s\\n' "$build" | sed -n 's/.*--error-log-path=\\([^[:space:]]*\\).*/\\1/p' | head -n 1)"
pid_path="$(printf '%s\\n' "$build" | sed -n 's/.*--pid-path=\\([^[:space:]]*\\).*/\\1/p' | head -n 1)"
[ -n "$config_path" ] || config_path="/etc/nginx/nginx.conf"
config_dir="$(dirname "$config_path")"
[ -n "$error_log" ] || error_log="/var/log/nginx/error.log"
[ -n "$pid_path" ] || pid_path="/run/nginx.pid"
distro=unknown
if [ -f /etc/alpine-release ]; then
  distro=alpine
elif [ -f /etc/debian_version ]; then
  distro=debian
elif [ -f /etc/redhat-release ] || [ -f /etc/centos-release ] || [ -f /etc/fedora-release ]; then
  distro=rhel
fi
available_dir="$config_dir/sites-available"
enabled_dir="$config_dir/sites-enabled"
conf_dir="$config_dir/conf.d"
if [ -d "$available_dir" ] || [ -d "$enabled_dir" ]; then
  sites_layout=debian
else
  sites_layout=rhel
  available_dir=""
  enabled_dir=""
fi
modules="$(printf '%s\\n' "$build" | tr ' ' '\\n' | sed -n 's/^--with-\\(.*_module\\)$/\\1/p;s/^--add-module=\\(.*\\)$/\\1/p' | paste -sd ',' -)"
if command -v systemctl >/dev/null 2>&1; then
  is_running="$(systemctl is-active nginx 2>/dev/null | grep -qx active && printf true || printf false)"
else
  is_running="$(nginx -t >/dev/null 2>&1 && pgrep nginx >/dev/null 2>&1 && printf true || printf false)"
fi
emit found true
emit version "$version"
emit modules "$modules"
emit configPath "$config_path"
emit configDir "$config_dir"
emit errorLogPath "$error_log"
emit pidFile "$pid_path"
emit binaryPath "$bin"
emit distro "$distro"
emit sitesLayout "$sites_layout"
emit availableDir "$available_dir"
emit enabledDir "$enabled_dir"
emit confDir "$conf_dir"
emit logDir "/var/log/nginx"
emit isRunning "$is_running"
`.trim(),
  };
}

export function parseNginxDetectOutput(stdout: string): NginxInstallation | null {
  const values = new Map<string, string>();

  for (const line of stdout.split(/\r?\n/)) {
    const match = line.match(/^__SHELLDESK_NGINX_FIELD__\|([^|]+)\|(.*)$/);
    if (match) values.set(match[1], match[2]);
  }

  if (first(values, 'found') === 'false' || !values.has('version')) return null;

  const configDir = first(values, 'configDir', '/etc/nginx');

  return {
    version: first(values, 'version'),
    modules: first(values, 'modules').split(',').map((value) => value.trim()).filter(Boolean),
    configPath: first(values, 'configPath', `${configDir}/nginx.conf`),
    configDir,
    errorLogPath: first(values, 'errorLogPath', '/var/log/nginx/error.log'),
    pidFile: first(values, 'pidFile', '/run/nginx.pid'),
    binaryPath: first(values, 'binaryPath', 'nginx'),
    distro: parseDistro(first(values, 'distro')),
    sitesLayout: parseSitesLayout(first(values, 'sitesLayout')),
    availableDir: normalizeNullable(first(values, 'availableDir')),
    enabledDir: normalizeNullable(first(values, 'enabledDir')),
    confDir: first(values, 'confDir', `${configDir}/conf.d`),
    logDir: first(values, 'logDir', '/var/log/nginx'),
    isRunning: first(values, 'isRunning') === 'true',
  };
}

export function createNginxListConfigsCommand(installation: NginxInstallation, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported(nginxFileMarker);

  const availableDir = installation.availableDir ? shellSingleQuote(installation.availableDir) : "''";
  const enabledDir = installation.enabledDir ? shellSingleQuote(installation.enabledDir) : "''";
  const confDir = shellSingleQuote(installation.confDir);

  return {
    command: `
available_dir=${availableDir}
enabled_dir=${enabledDir}
conf_dir=${confDir}
emit_file() {
  file="$1"; enabled="$2"
  [ -f "$file" ] || return 0
  size="$(stat -c %s "$file" 2>/dev/null || wc -c < "$file" 2>/dev/null || printf 0)"
  mtime="$(stat -c %Y "$file" 2>/dev/null || printf 0)"
  printf '${nginxFileMarker}|%s|%s|%s|%s\\n' "$file" "$enabled" "$size" "$mtime"
}
is_enabled_debian() {
  file="$1"
  [ -n "$enabled_dir" ] && [ -e "$enabled_dir/$(basename "$file")" ]
}
if [ -n "$available_dir" ] && [ -d "$available_dir" ]; then
  find "$available_dir" -maxdepth 1 -type f -name '*.conf' 2>/dev/null | sort | while IFS= read -r file; do
    if is_enabled_debian "$file"; then emit_file "$file" true; else emit_file "$file" false; fi
  done
fi
if [ -d "$conf_dir" ]; then
  find "$conf_dir" -maxdepth 1 -type f \\( -name '*.conf' -o -name '*.conf.disabled' \\) 2>/dev/null | sort | while IFS= read -r file; do
    case "$file" in
      *.disabled) emit_file "$file" false ;;
      *) emit_file "$file" true ;;
    esac
  done
fi
`.trim(),
  };
}

export function parseNginxListConfigs(stdout: string): { path: string; enabled: boolean; size: number; mtime: number }[] {
  return stdout
    .split(/\r?\n/)
    .map((line) => line.match(/^__SHELLDESK_NGINX_FILE__\|([^|]+)\|(true|false)\|(\d+)\|(\d+)$/))
    .filter((match): match is RegExpMatchArray => Boolean(match))
    .map((match) => ({
      path: match[1],
      enabled: match[2] === 'true',
      size: Number(match[3]),
      mtime: Number(match[4]),
    }));
}

export function createNginxReadConfigCommand(filePath: string, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  return {
    command: `cat -- ${shellSingleQuote(filePath)} 2>/dev/null || sudo -n cat -- ${shellSingleQuote(filePath)}`,
  };
}

export function createNginxEnableSiteCommand(filename: string, installation: NginxInstallation, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  const safeFilename = basename(filename);
  if (installation.sitesLayout === 'debian' && installation.availableDir && installation.enabledDir) {
    const source = `${installation.availableDir}/${safeFilename}`;
    const target = `${installation.enabledDir}/${safeFilename}`;
    return { command: sudoCommand(`ln -sf ${shellSingleQuote(source)} ${shellSingleQuote(target)}`) };
  }

  const disabled = `${installation.confDir}/${safeFilename.replace(/\.conf$/i, '')}.conf.disabled`;
  const enabled = `${installation.confDir}/${safeFilename.replace(/\.disabled$/i, '')}`;
  return { command: sudoCommand(`mv ${shellSingleQuote(disabled)} ${shellSingleQuote(enabled)}`) };
}

export function createNginxDisableSiteCommand(filePath: string, installation: NginxInstallation, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  const safeFilename = basename(filePath);
  if (installation.sitesLayout === 'debian' && installation.enabledDir) {
    return { command: sudoCommand(`rm -f -- ${shellSingleQuote(`${installation.enabledDir}/${safeFilename}`)}`) };
  }

  const target = filePath.endsWith('.disabled') ? filePath : `${filePath}.disabled`;
  return { command: sudoCommand(`mv ${shellSingleQuote(filePath)} ${shellSingleQuote(target)}`) };
}

export function createNginxBackupCommand(filePath: string, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  return {
    command: sudoCommand(`cp -- ${shellSingleQuote(filePath)} ${shellSingleQuote(`${filePath}.bak`)}.$(date +%Y%m%d%H%M%S)`),
  };
}

export function createNginxWriteConfigCommand(filePath: string, content: string, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  const delimiter = createHeredocDelimiter(content);

  return {
    command: `
tmp="$(mktemp 2>/dev/null || printf "/tmp/shelldesk-nginx-write-$$")"
trap 'rm -f -- "$tmp"' EXIT HUP INT TERM
cat > "$tmp" <<'${delimiter}'
${content}
${delimiter}
if [ "$(id -u 2>/dev/null)" = "0" ]; then
  install -m 0644 "$tmp" ${shellSingleQuote(filePath)}
else
  sudo -n install -m 0644 "$tmp" ${shellSingleQuote(filePath)}
fi
`.trim(),
  };
}

export function createNginxTestCommand(isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  return { command: 'sudo -n nginx -t 2>&1 || nginx -t 2>&1' };
}

export function createNginxReloadCommand(isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  return { command: 'sudo -n systemctl reload nginx 2>&1 || sudo -n nginx -s reload 2>&1 || nginx -s reload 2>&1' };
}

export function createNginxDeleteCommand(filePath: string, installation: NginxInstallation, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  const backupDir = `${installation.configDir}/.shelldesk-backups`;
  const backupPrefix = `${backupDir}/${basename(filePath)}.`;
  const unlinkEnabled = installation.enabledDir
    ? `rm -f -- ${shellSingleQuote(`${installation.enabledDir}/${basename(filePath).replace(/\.disabled$/i, '')}`)}; `
    : '';

  return {
    command: sudoCommand(`mkdir -p ${shellSingleQuote(backupDir)}; ${unlinkEnabled}mv -- ${shellSingleQuote(filePath)} ${shellSingleQuote(backupPrefix)}$(date +%Y%m%d%H%M%S)`),
  };
}

export function createNginxListBackupsCommand(configDir: string, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  return { command: `ls -la -- ${shellSingleQuote(`${configDir}/.shelldesk-backups`)} 2>&1` };
}

export function createNginxRestoreBackupCommand(backupPath: string, originalPath: string, isWindowsHost = false): RemoteCommandInput {
  if (isWindowsHost) return windowsUnsupported();

  return { command: sudoCommand(`cp -- ${shellSingleQuote(backupPath)} ${shellSingleQuote(originalPath)}`) };
}

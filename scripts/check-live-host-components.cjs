const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const dotenvPath = path.join(root, '.env');
const requiredKeys = ['SHELLDESK_TEST_SSH_HOST', 'SHELLDESK_TEST_SSH_USERNAME'];

function parseDotenv(text) {
  const values = {};
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const separator = trimmed.indexOf('=');
    if (separator < 0) continue;
    const key = trimmed.slice(0, separator).trim();
    const value = trimmed.slice(separator + 1).trim();
    values[key] = value;
  }
  return values;
}

const values = fs.existsSync(dotenvPath)
  ? parseDotenv(fs.readFileSync(dotenvPath, 'utf8'))
  : {};
const missing = requiredKeys.filter((key) => !values[key] || values[key] === 'change-me');
const hasPassword = Boolean(values.SHELLDESK_TEST_SSH_PASSWORD && values.SHELLDESK_TEST_SSH_PASSWORD !== 'change-me');
const hasKey = Boolean(values.SHELLDESK_TEST_SSH_KEY_PATH && values.SHELLDESK_TEST_SSH_KEY_PATH !== 'change-me');
if (!hasPassword && !hasKey) missing.push('SHELLDESK_TEST_SSH_PASSWORD or SHELLDESK_TEST_SSH_KEY_PATH');
if (missing.length) {
  console.error(`Live host smoke requires .env values: ${missing.join(', ')}`);
  process.exit(1);
}

const password = values.SHELLDESK_TEST_SSH_PASSWORD;
const keyPath = values.SHELLDESK_TEST_SSH_KEY_PATH;
const cargo = process.platform === 'win32' ? 'cargo.exe' : 'cargo';
const result = spawnSync(cargo, [
  'test',
  '--manifest-path',
  'src-tauri/Cargo.toml',
  'live_host_components_smoke',
  '--',
  '--nocapture',
], {
  cwd: root,
  env: {
    ...process.env,
    SHELLDESK_RUN_LIVE_HOST_COMPONENTS: '1',
  },
  encoding: 'utf8',
  stdio: 'pipe',
});

function redact(text) {
  let result = text;
  if (password) result = result.replaceAll(password, '[redacted]');
  if (keyPath) result = result.replaceAll(keyPath, '[key-path]');
  return result;
}

if (result.stdout) process.stdout.write(redact(result.stdout));
if (result.stderr) process.stderr.write(redact(result.stderr));
if (result.status !== 0) process.exit(result.status ?? 1);

console.log('Live host component smoke passed.');

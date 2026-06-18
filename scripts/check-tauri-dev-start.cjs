const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const timeoutMs = Number.parseInt(process.env.SHELLDESK_TAURI_SMOKE_TIMEOUT_MS || '120000', 10);
const logPath = path.join(os.tmpdir(), `shelldesk-tauri-dev-smoke-${Date.now()}.log`);
const command = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';

let output = '';
let settled = false;
let sawRunLine = false;
const startedAt = Date.now();

function stripAnsi(value) {
  return value.replace(/\x1B\[[0-?]*[ -/]*[@-~]/g, '');
}

function relevantErrors(text) {
  const preRunText = sawRunLine
    ? text.split(/Running.*target[\\/]+debug[\\/]+shelldesk(?:\.exe)?/i)[0] || text
    : text;

  return stripAnsi(preRunText)
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .filter((line) => (
      /\b(error|failed|panic|thread .* panicked)\b/i.test(line) &&
      !/\b(warning|warn|0 errors)\b/i.test(line)
    ));
}

function stopProcessTree(child) {
  if (!child.pid) return;

  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    return;
  }

  try {
    process.kill(-child.pid, 'SIGTERM');
  } catch {
    try {
      process.kill(child.pid, 'SIGTERM');
    } catch {
      // Process already exited.
    }
  }
}

function finish(child, exitCode, message) {
  if (settled) return;
  settled = true;
  clearTimeout(timeout);
  stopProcessTree(child);
  fs.writeFileSync(logPath, output);
  if (message) {
    const elapsed = ((Date.now() - startedAt) / 1000).toFixed(1);
    const stream = exitCode === 0 ? process.stdout : process.stderr;
    stream.write(`${message}\n`);
    stream.write(`Tauri dev startup smoke log: ${logPath}\n`);
    stream.write(`Elapsed: ${elapsed}s\n`);
  }
  process.exit(exitCode);
}

const child = spawn(command, ['start'], {
  cwd: root,
  env: {
    ...process.env,
    FORCE_COLOR: '0',
  },
  detached: process.platform !== 'win32',
  stdio: ['ignore', 'pipe', 'pipe'],
  shell: process.platform === 'win32',
});

const timeout = setTimeout(() => {
  const errors = relevantErrors(output);
  const details = errors.length ? `\nFirst startup errors:\n${errors.slice(0, 5).join('\n')}` : '';
  finish(child, 1, `Tauri dev startup smoke timed out before the desktop binary launched.${details}`);
}, timeoutMs);

function handleChunk(chunk) {
  output += chunk.toString();
  sawRunLine = /Running.*target[\\/]+debug[\\/]+shelldesk(?:\.exe)?/i.test(stripAnsi(output));
  const errors = relevantErrors(output);

  if (errors.length) {
    finish(child, 1, `Tauri dev startup smoke failed before app launch.\nFirst startup errors:\n${errors.slice(0, 5).join('\n')}`);
    return;
  }

  if (sawRunLine) {
    finish(child, 0, 'Tauri dev startup smoke passed: desktop binary launch was observed.');
  }
}

child.stdout.on('data', handleChunk);
child.stderr.on('data', handleChunk);

child.on('error', (error) => {
  output += `\nFailed to spawn ${command}: ${error.message}\n`;
  finish(child, 1, `Tauri dev startup smoke failed to spawn ${command}: ${error.message}`);
});

child.on('exit', (code, signal) => {
  if (settled) return;
  const errors = relevantErrors(output);
  const detail = errors.length ? `\nFirst startup errors:\n${errors.slice(0, 5).join('\n')}` : '';
  finish(child, 1, `Tauri dev startup smoke exited before desktop binary launch. code=${code} signal=${signal}${detail}`);
});

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const updaterEndpoint = 'https://github.com/liubaicai/ShellDesk/releases/latest/download/latest.json';
const args = process.argv.slice(2);
const isDebugBuild = args.includes('--debug') || args.includes('-d');
const publicKey = (process.env.TAURI_UPDATER_PUBLIC_KEY || '').trim();

const config = {};

if (publicKey) {
  config.plugins = {
    updater: {
      pubkey: publicKey,
      endpoints: [updaterEndpoint],
    },
  };
} else if (isDebugBuild) {
  config.bundle = {
    createUpdaterArtifacts: false,
  };
} else {
  console.error('TAURI_UPDATER_PUBLIC_KEY is required for release packaging.');
  console.error('Use pnpm pack:dir for a local debug bundle that does not create updater artifacts.');
  process.exit(1);
}

const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'shelldesk-tauri-config-'));
const configPath = path.join(tempDir, 'tauri.build.conf.json');
let exitCode = 1;

try {
  fs.writeFileSync(configPath, JSON.stringify(config, null, 2));

  const command = process.env.npm_execpath || (process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm');
  if (process.env.SHELLDESK_TAURI_BUILD_DRY_RUN === '1') {
    console.log(JSON.stringify({
      command,
      args: ['tauri', 'build', ...args, '--config', configPath],
      config,
    }, null, 2));
    exitCode = 0;
  } else {
    const result = spawnSync(command, ['tauri', 'build', ...args, '--config', configPath], {
      cwd: path.resolve(__dirname, '..'),
      env: process.env,
      stdio: 'inherit',
      shell: process.platform === 'win32',
    });

    if (result.error) {
      console.error(`Failed to start Tauri build: ${result.error.message}`);
    }
    exitCode = result.status ?? 1;
  }
} finally {
  fs.rmSync(tempDir, { recursive: true, force: true });
}

process.exit(exitCode);

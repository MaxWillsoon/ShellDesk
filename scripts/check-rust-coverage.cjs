#!/usr/bin/env node

const { spawnSync } = require('node:child_process');

const manifestArgs = ['--manifest-path', 'src-tauri/Cargo.toml'];
const version = spawnSync('cargo', ['llvm-cov', '--version'], {
  encoding: 'utf8',
  shell: process.platform === 'win32',
});

if (version.status !== 0) {
  console.error('cargo-llvm-cov is not installed.');
  console.error('Install it with: cargo install cargo-llvm-cov');
  process.exit(version.status || 1);
}

const args = [
  'llvm-cov',
  ...manifestArgs,
  '--workspace',
  '--all-features',
  '--summary-only',
];
const result = spawnSync('cargo', args, {
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

process.exit(result.status || 0);

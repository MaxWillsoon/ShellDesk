#!/usr/bin/env node

const { spawnSync } = require('node:child_process');
const { chmodSync, existsSync } = require('node:fs');
const path = require('node:path');

const repoRootResult = spawnSync('git', ['rev-parse', '--show-toplevel'], {
  encoding: 'utf8',
});

if (repoRootResult.status !== 0) {
  console.warn('Skipping git hook installation because this is not a git working tree.');
  process.exit(0);
}

const repoRoot = repoRootResult.stdout.trim();
const hooksPath = '.githooks';
const configResult = spawnSync('git', ['config', 'core.hooksPath', hooksPath], {
  cwd: repoRoot,
  stdio: 'inherit',
});

if (configResult.status !== 0) {
  process.exit(configResult.status ?? 1);
}

const preCommitHook = path.join(repoRoot, hooksPath, 'pre-commit');

if (existsSync(preCommitHook)) {
  try {
    chmodSync(preCommitHook, 0o755);
  } catch (error) {
    console.warn(`Git hooks path was configured, but chmod failed: ${error.message}`);
  }
}

console.log(`Git hooks installed from ${hooksPath}`);

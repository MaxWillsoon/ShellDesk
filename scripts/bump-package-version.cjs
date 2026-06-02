#!/usr/bin/env node

const { readFileSync, writeFileSync } = require('node:fs');
const path = require('node:path');

const packageJsonPath = path.resolve(__dirname, '..', 'package.json');
const packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf8'));
const currentVersion = packageJson.version;

if (typeof currentVersion !== 'string') {
  throw new Error('package.json version must be a string.');
}

const versionMatch = currentVersion.match(/^(\d+)\.(\d+)\.(\d+)$/);

if (!versionMatch) {
  throw new Error(`package.json version must use major.minor.patch format, received "${currentVersion}".`);
}

const [, major, minor, patch] = versionMatch;
const nextVersion = `${major}.${minor}.${Number.parseInt(patch, 10) + 1}`;

packageJson.version = nextVersion;
writeFileSync(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`);

console.log(`ShellDesk version bumped: ${currentVersion} -> ${nextVersion}`);

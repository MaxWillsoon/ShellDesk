#!/usr/bin/env node

const { spawnSync } = require('node:child_process');
const { readFileSync } = require('node:fs');
const path = require('node:path');

const packageJsonPath = path.resolve(__dirname, '..', 'package.json');
const packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf8'));
const version = packageJson.version;

if (typeof version !== 'string') {
  throw new Error('package.json version must be a string.');
}

const versionMatch = version.match(
  /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[A-Za-z][0-9A-Za-z-]*|[0-9A-Za-z][0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[A-Za-z][0-9A-Za-z-]*|[0-9A-Za-z][0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*))?$/
);

if (!versionMatch) {
  throw new Error(`package.json version must be a valid release version, received "${version}".`);
}

const tag = `v${version}`;

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    stdio: options.stdio ?? 'pipe',
    ...options,
  });

  return result;
}

const worktreeResult = run('git', ['rev-parse', '--show-toplevel']);

if (worktreeResult.status !== 0) {
  throw new Error('This command must be run inside a git working tree.');
}

const packageStatusResult = run('git', ['status', '--porcelain', '--', 'package.json']);

if (packageStatusResult.status !== 0) {
  process.exit(packageStatusResult.status ?? 1);
}

if (packageStatusResult.stdout.trim()) {
  throw new Error('package.json has uncommitted changes. Commit the version first, then create the tag.');
}

const localTagResult = run('git', ['rev-parse', '--verify', '--quiet', `refs/tags/${tag}`]);

if (localTagResult.status === 0) {
  throw new Error(`Tag ${tag} already exists locally.`);
}

const remoteTagResult = run('git', ['ls-remote', '--exit-code', '--tags', 'origin', `refs/tags/${tag}`]);

if (remoteTagResult.status === 0) {
  throw new Error(`Tag ${tag} already exists on origin.`);
}

console.log(`Creating tag ${tag} from package.json version ${version}`);

const createTagResult = run('git', ['tag', tag], { stdio: 'inherit' });

if (createTagResult.status !== 0) {
  process.exit(createTagResult.status ?? 1);
}

const pushTagResult = run('git', ['push', 'origin', tag], { stdio: 'inherit' });

if (pushTagResult.status !== 0) {
  console.error(`Failed to push ${tag}. The tag was created locally; delete it with "git tag -d ${tag}" if needed.`);
  process.exit(pushTagResult.status ?? 1);
}

console.log(`Pushed ${tag} to origin.`);

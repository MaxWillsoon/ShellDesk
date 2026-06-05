#!/usr/bin/env node

const { spawnSync } = require('node:child_process');

const VERSION_PATTERN = '(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)';
const STABLE_VERSION_RE = new RegExp(`^v?${VERSION_PATTERN}$`);
const RELEASE_TAG_RE = new RegExp(
  `^v${VERSION_PATTERN}(-((0|[1-9][0-9]*|[A-Za-z][0-9A-Za-z-]*|[0-9A-Za-z][0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\\.(0|[1-9][0-9]*|[A-Za-z][0-9A-Za-z-]*|[0-9A-Za-z][0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*))?$`
);

function runGit(args, options = {}) {
  return spawnSync('git', args, {
    encoding: 'utf8',
    stdio: options.stdio ?? 'pipe',
    ...options,
  });
}

function assertGitSuccess(result, message) {
  if (result.status === 0) {
    return;
  }

  const detail = [result.stderr, result.stdout]
    .filter((value) => typeof value === 'string' && value.trim())
    .join('\n')
    .trim();
  throw new Error(detail ? `${message}\n${detail}` : message);
}

function normalizeRequestedTag(value) {
  const trimmedValue = value.trim();
  const tag = trimmedValue.startsWith('v') ? trimmedValue : `v${trimmedValue}`;

  if (!RELEASE_TAG_RE.test(tag)) {
    throw new Error(`Invalid release tag "${value}". Use v<MAJOR>.<MINOR>.<PATCH> or v<MAJOR>.<MINOR>.<PATCH>-<prerelease>.`);
  }

  return tag;
}

function parseStableVersionTag(value) {
  const tagName = value
    .trim()
    .replace(/^refs\/tags\//, '')
    .replace(/\^\{\}$/, '');
  const match = tagName.match(STABLE_VERSION_RE);

  if (!match) {
    return null;
  }

  return {
    tag: tagName,
    major: Number.parseInt(match[1], 10),
    minor: Number.parseInt(match[2], 10),
    patch: Number.parseInt(match[3], 10),
  };
}

function compareStableVersions(left, right) {
  return left.major - right.major || left.minor - right.minor || left.patch - right.patch;
}

function computeNextTag(tags) {
  const versions = tags
    .map(parseStableVersionTag)
    .filter(Boolean)
    .sort(compareStableVersions);

  if (versions.length === 0) {
    return 'v0.0.1';
  }

  const latest = versions[versions.length - 1];
  return `v${latest.major}.${latest.minor}.${latest.patch + 1}`;
}

function readLocalTags() {
  const result = runGit(['tag', '--list']);
  assertGitSuccess(result, 'Failed to read local git tags.');
  return result.stdout.split(/\r?\n/).filter(Boolean);
}

function readRemoteTags() {
  const remoteResult = runGit(['remote', 'get-url', 'origin']);
  assertGitSuccess(remoteResult, 'Git remote "origin" is not configured.');

  const result = runGit(['ls-remote', '--tags', 'origin']);
  assertGitSuccess(result, 'Failed to read tags from origin.');

  return result.stdout
    .split(/\r?\n/)
    .map((line) => line.trim().split(/\s+/)[1])
    .filter(Boolean);
}

function assertCleanWorktree() {
  const result = runGit(['status', '--porcelain']);
  assertGitSuccess(result, 'Failed to read git working tree status.');

  if (result.stdout.trim()) {
    throw new Error('Working tree has uncommitted changes. Commit or stash them before creating a release tag.');
  }
}

function assertTagDoesNotExist(tag) {
  const localResult = runGit(['rev-parse', '--verify', '--quiet', `refs/tags/${tag}`]);

  if (localResult.status === 0) {
    throw new Error(`Tag ${tag} already exists locally.`);
  }

  const remoteResult = runGit(['ls-remote', '--exit-code', '--tags', 'origin', `refs/tags/${tag}`]);

  if (remoteResult.status === 0) {
    throw new Error(`Tag ${tag} already exists on origin.`);
  }

  if (remoteResult.status !== 2) {
    assertGitSuccess(remoteResult, `Failed to check whether ${tag} exists on origin.`);
  }
}

function resolveTargetTag(args) {
  if (args.length > 1) {
    throw new Error('Usage: pnpm run tag [v<MAJOR>.<MINOR>.<PATCH>]');
  }

  if (args.length === 1) {
    return normalizeRequestedTag(args[0]);
  }

  return computeNextTag([...readLocalTags(), ...readRemoteTags()]);
}

function main() {
  const worktreeResult = runGit(['rev-parse', '--show-toplevel']);
  assertGitSuccess(worktreeResult, 'This command must be run inside a git working tree.');

  const headResult = runGit(['rev-parse', '--verify', 'HEAD']);
  assertGitSuccess(headResult, 'Cannot create a tag because HEAD is not a valid commit.');

  assertCleanWorktree();

  const tag = resolveTargetTag(process.argv.slice(2));
  assertTagDoesNotExist(tag);

  console.log(`Creating ${tag} at ${headResult.stdout.trim()}`);
  const tagResult = runGit(['tag', tag], { stdio: 'inherit' });
  assertGitSuccess(tagResult, `Failed to create ${tag}.`);

  console.log(`Pushing ${tag} to origin`);
  const pushResult = runGit(['push', 'origin', tag], { stdio: 'inherit' });

  if (pushResult.status !== 0) {
    console.error(`Failed to push ${tag}. The tag was created locally; delete it with "git tag -d ${tag}" if needed.`);
    process.exit(pushResult.status ?? 1);
  }

  console.log(`Pushed ${tag} to origin.`);
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}

module.exports = {
  computeNextTag,
  normalizeRequestedTag,
  parseStableVersionTag,
};

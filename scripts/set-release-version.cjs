const fs = require('node:fs');
const path = require('node:path');

const args = process.argv.slice(2);
const version = args.find((arg) => !arg.startsWith('-')) || process.env.VERSION;
const checkOnly = args.includes('--check');
const versionRe = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((0|[1-9]\d*|[A-Za-z][0-9A-Za-z-]*|[0-9A-Za-z][0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9]\d*|[A-Za-z][0-9A-Za-z-]*|[0-9A-Za-z][0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*))?$/;

if (!version || !versionRe.test(version)) {
  console.error('Usage: node scripts/set-release-version.cjs <semver> [--check]');
  console.error('Example: node scripts/set-release-version.cjs 1.2.3');
  process.exit(1);
}

function readText(filePath) {
  return fs.readFileSync(filePath, 'utf8');
}

function writeText(filePath, content) {
  if (!checkOnly) {
    fs.writeFileSync(filePath, content);
  }
}

function updateJsonVersion(filePath) {
  const absolutePath = path.resolve(filePath);
  const json = JSON.parse(readText(absolutePath));
  const current = json.version;
  json.version = version;
  const next = `${JSON.stringify(json, null, 2)}\n`;
  return { filePath, current, next, absolutePath };
}

function updateCargoTomlVersion(filePath) {
  const absolutePath = path.resolve(filePath);
  const currentText = readText(absolutePath);
  const packageHeader = currentText.match(/(^|\n)\[package\]\r?\n/);
  if (!packageHeader) {
    throw new Error(`Missing [package] section in ${filePath}`);
  }
  const start = packageHeader.index + packageHeader[0].length;
  const nextSection = currentText.slice(start).search(/\r?\n\[/);
  const end = nextSection >= 0 ? start + nextSection : currentText.length;
  const before = currentText.slice(0, start);
  const packageBody = currentText.slice(start, end);
  const after = currentText.slice(end);
  const match = packageBody.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error(`Missing package version in ${filePath}`);
  }
  const nextBody = packageBody.replace(/^version\s*=\s*"([^"]+)"/m, `version = "${version}"`);
  return {
    filePath,
    current: match[1],
    next: `${before}${nextBody}${after}`,
    absolutePath,
  };
}

function updateCargoLockVersion(filePath, packageName) {
  const absolutePath = path.resolve(filePath);
  if (!fs.existsSync(absolutePath)) {
    return null;
  }
  const currentText = readText(absolutePath);
  const packageBlocks = currentText.split(/(?=^\[\[package\]\]$)/m);
  let updated = false;
  let current = null;
  const packageNameRe = new RegExp(`^name\\s*=\\s*"${packageName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}"$`, 'm');
  const next = packageBlocks.map((block) => {
    if (!packageNameRe.test(block)) {
      return block;
    }
    const versionMatch = block.match(/^version\s*=\s*"([^"]+)"/m);
    if (!versionMatch) {
      throw new Error(`Missing lockfile version for ${packageName} in ${filePath}`);
    }
    current = versionMatch[1];
    updated = true;
    return block.replace(/^version\s*=\s*"([^"]+)"/m, `version = "${version}"`);
  }).join('');
  if (!updated) {
    throw new Error(`Missing ${packageName} package in ${filePath}`);
  }
  return { filePath, current, next, absolutePath };
}

const changes = [
  updateJsonVersion('package.json'),
  updateJsonVersion('src-tauri/tauri.conf.json'),
  updateCargoTomlVersion('src-tauri/Cargo.toml'),
  updateCargoLockVersion('src-tauri/Cargo.lock', 'shelldesk'),
].filter(Boolean);

const mismatches = changes.filter((change) => change.current !== version);

if (checkOnly) {
  if (mismatches.length) {
    for (const change of mismatches) {
      console.error(`${change.filePath}: expected ${version}, found ${change.current}`);
    }
    process.exit(1);
  }
  console.log(`Release version check ok: ${version}`);
  process.exit(0);
}

for (const change of changes) {
  writeText(change.absolutePath, change.next);
  const status = change.current === version ? 'kept' : 'updated';
  console.log(`${status} ${change.filePath}: ${change.current} -> ${version}`);
}

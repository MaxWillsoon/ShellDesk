const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const repoRoot = path.resolve(__dirname, '..');
const backupRoot = path.join(repoRoot, 'backup');

const recursiveExactPaths = [
  'src/assets',
  'docs/images',
  'public',
];

const exactFiles = [
  'src/i18n.ts',
  'src/i18nCoreCatalog.ts',
  'src/fontUtils.ts',
  'index.html',
  'vite.config.ts',
  'tsconfig.json',
];

const i18nCatalogAllowedChanges = [
  {
    from: "'terminal.message.legacyMode': '检测到旧版 Electron 主进程，使用单终端兼容模式。'",
    to: "'terminal.message.legacyMode': '检测到旧版后端能力，使用单终端兼容模式。'",
  },
  {
    from: "'terminal.message.legacyMode': 'Detected an older Electron main process. Using single-terminal compatibility mode.'",
    to: "'terminal.message.legacyMode': 'Detected older backend capabilities. Using single-terminal compatibility mode.'",
  },
];

const removedNodeBackendPackages = [
  'cpu-features',
  'electron',
  'electron-winstaller',
  'ssh2',
];

function workspacePath(relativePath) {
  return path.join(repoRoot, relativePath);
}

function backupPath(relativePath) {
  return path.join(backupRoot, relativePath);
}

function fileHash(filePath) {
  return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function listFiles(directory, root = directory) {
  if (!fs.existsSync(directory)) {
    return [];
  }

  return fs.readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        return listFiles(fullPath, root);
      }
      return entry.isFile() ? [path.relative(root, fullPath).replace(/\\/g, '/')] : [];
    })
    .sort((left, right) => left.localeCompare(right));
}

function assertSameFile(relativePath) {
  const currentFile = workspacePath(relativePath);
  const backupFile = backupPath(relativePath);
  assertExists(currentFile, `Current file is missing: ${relativePath}`);
  assertExists(backupFile, `Backup file is missing: ${relativePath}`);
  if (fileHash(currentFile) !== fileHash(backupFile)) {
    throw new Error(`Migration fidelity check failed: ${relativePath} differs from backup.`);
  }
}

function assertExists(filePath, message) {
  if (!fs.existsSync(filePath)) {
    throw new Error(message);
  }
}

function assertSameTree(relativePath) {
  const currentRoot = workspacePath(relativePath);
  const previousRoot = backupPath(relativePath);
  const currentFiles = listFiles(currentRoot);
  const previousFiles = listFiles(previousRoot);
  const currentSet = new Set(currentFiles);
  const previousSet = new Set(previousFiles);
  const missing = previousFiles.filter((file) => !currentSet.has(file));
  const extra = currentFiles.filter((file) => !previousSet.has(file));
  const changed = previousFiles.filter((file) => currentSet.has(file) && fileHash(path.join(currentRoot, file)) !== fileHash(path.join(previousRoot, file)));

  if (missing.length || extra.length || changed.length) {
    throw new Error([
      `Migration fidelity check failed for ${relativePath}.`,
      missing.length ? `Missing: ${missing.join(', ')}` : '',
      extra.length ? `Extra: ${extra.join(', ')}` : '',
      changed.length ? `Changed: ${changed.join(', ')}` : '',
    ].filter(Boolean).join('\n'));
  }
}

function normalizeAllowedI18nCatalog(text) {
  return i18nCatalogAllowedChanges.reduce(
    (next, change) => next.replaceAll(change.from, change.to),
    text.replace(/\r\n/g, '\n'),
  ).replace(/\r\n/g, '\n');
}

function assertI18nCatalogMatchesAllowedMigration() {
  const currentPath = workspacePath('src/i18nCatalog.ts');
  const backupCatalogPath = backupPath('src/i18nCatalog.ts');
  assertExists(currentPath, 'Current i18n catalog is missing.');
  assertExists(backupCatalogPath, 'Backup i18n catalog is missing.');

  const currentText = fs.readFileSync(currentPath, 'utf8').replace(/\r\n/g, '\n');
  const expectedText = normalizeAllowedI18nCatalog(fs.readFileSync(backupCatalogPath, 'utf8'));
  if (currentText !== expectedText) {
    throw new Error('Migration fidelity check failed: src/i18nCatalog.ts has changes outside the allowed Electron backend wording migration.');
  }
}

function assertElectronBackendPackagesRemoved() {
  const packageJson = JSON.parse(fs.readFileSync(workspacePath('package.json'), 'utf8'));
  const packageSections = [
    ['dependencies', packageJson.dependencies || {}],
    ['devDependencies', packageJson.devDependencies || {}],
    ['optionalDependencies', packageJson.optionalDependencies || {}],
  ];
  const packageFailures = packageSections.flatMap(([section, dependencies]) => (
    removedNodeBackendPackages
      .filter((packageName) => Object.prototype.hasOwnProperty.call(dependencies, packageName))
      .map((packageName) => `package.json ${section}.${packageName}`)
  ));

  const workspaceYaml = fs.readFileSync(workspacePath('pnpm-workspace.yaml'), 'utf8');
  const workspaceFailures = removedNodeBackendPackages
    .filter((packageName) => new RegExp(`^\\s*-\\s*['"]?${packageName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}['"]?\\s*$`, 'm').test(workspaceYaml))
    .map((packageName) => `pnpm-workspace.yaml onlyBuiltDependencies ${packageName}`);

  const failures = [...packageFailures, ...workspaceFailures];
  if (failures.length) {
    throw new Error([
      'Migration fidelity check failed: Electron/Node SSH backend packages should not be present in the Tauri project.',
      ...failures.map((failure) => `  - ${failure}`),
    ].join('\n'));
  }
}

for (const relativePath of recursiveExactPaths) {
  assertSameTree(relativePath);
}

for (const relativePath of exactFiles) {
  assertSameFile(relativePath);
}

assertI18nCatalogMatchesAllowedMigration();
assertElectronBackendPackagesRemoved();

console.log('Migration fidelity ok: assets, public files, core config, i18n, and removed Electron/Node SSH backend packages match the Tauri migration contract.');

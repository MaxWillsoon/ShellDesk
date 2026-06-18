const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const root = path.resolve(__dirname, '..');
const currentRoot = path.join(root, 'src');
const backupRoot = path.join(root, 'backup/src');
const checkedExtensions = new Set(['.ts', '.tsx', '.scss']);

const allowedChangedFiles = new Set([
  'i18nCatalog.ts',
  'main.tsx',
  'RemoteDesktopShell.tsx',
  'components/remote-desktop/RemoteBrowser.tsx',
  'vite-env.d.ts',
]);

const allowedExtraFiles = new Set([
  'tauriBridge.ts',
]);

function listFiles(directory, rootDirectory = directory) {
  return fs.readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        return listFiles(fullPath, rootDirectory);
      }
      if (!entry.isFile() || !checkedExtensions.has(path.extname(entry.name))) {
        return [];
      }
      return [path.relative(rootDirectory, fullPath).replace(/\\/g, '/')];
    })
    .sort((left, right) => left.localeCompare(right));
}

function normalizedText(filePath) {
  return fs.readFileSync(filePath, 'utf8')
    .replace(/\r\n/g, '\n')
    .replace(/[ \t]+$/gm, '')
    .replace(/\s+$/u, '\n');
}

function fileHash(filePath) {
  return crypto.createHash('sha256').update(normalizedText(filePath)).digest('hex');
}

function currentPath(relativePath) {
  return path.join(currentRoot, relativePath);
}

function backupPath(relativePath) {
  return path.join(backupRoot, relativePath);
}

const currentFiles = listFiles(currentRoot);
const backupFiles = listFiles(backupRoot);
const currentSet = new Set(currentFiles);
const backupSet = new Set(backupFiles);

const missing = backupFiles.filter((file) => !currentSet.has(file));
const extra = currentFiles.filter((file) => !backupSet.has(file) && !allowedExtraFiles.has(file));
const changed = backupFiles.filter((file) => (
  currentSet.has(file) &&
  fileHash(currentPath(file)) !== fileHash(backupPath(file)) &&
  !allowedChangedFiles.has(file)
));

const unexpectedAllowedMissing = [...allowedChangedFiles].filter((file) => (
  !currentSet.has(file) || !backupSet.has(file)
));
const unexpectedExtraMissing = [...allowedExtraFiles].filter((file) => !currentSet.has(file));

const failures = [];
if (missing.length) {
  failures.push(`Missing frontend files copied from backup:\n${missing.map((file) => `  - ${file}`).join('\n')}`);
}
if (extra.length) {
  failures.push(`Unexpected extra frontend files outside the Tauri migration allowlist:\n${extra.map((file) => `  - ${file}`).join('\n')}`);
}
if (changed.length) {
  failures.push(`Frontend files differ from backup outside the Tauri migration allowlist:\n${changed.map((file) => `  - ${file}`).join('\n')}`);
}
if (unexpectedAllowedMissing.length) {
  failures.push(`Allowed changed files are missing in current or backup trees:\n${unexpectedAllowedMissing.map((file) => `  - ${file}`).join('\n')}`);
}
if (unexpectedExtraMissing.length) {
  failures.push(`Allowed Tauri-only frontend files are missing:\n${unexpectedExtraMissing.map((file) => `  - ${file}`).join('\n')}`);
}

const mainSource = normalizedText(currentPath('main.tsx'));
if (!mainSource.includes("import './tauriBridge';")) {
  failures.push('src/main.tsx must import ./tauriBridge before mounting the React app.');
}

const bridgeSource = normalizedText(currentPath('tauriBridge.ts'));
if (!bridgeSource.includes('window.guiSSH')) {
  failures.push('src/tauriBridge.ts must expose the legacy window.guiSSH API surface.');
}
if (!bridgeSource.includes("invoke<T>('ipc_dispatch'")) {
  failures.push('src/tauriBridge.ts must route renderer IPC through the Tauri ipc_dispatch command.');
}
if (!bridgeSource.includes('function isTauriRuntime()') || !bridgeSource.includes('__TAURI_INTERNALS__')) {
  failures.push('src/tauriBridge.ts must detect the Tauri runtime before using native APIs.');
}
if (!bridgeSource.includes('async function previewIpc') || !bridgeSource.includes('createPreviewVaultSnapshot')) {
  failures.push('src/tauriBridge.ts must keep a browser-preview fallback for UI smoke testing.');
}

const browserSource = normalizedText(currentPath('components/remote-desktop/RemoteBrowser.tsx'));
if (!browserSource.includes('HTMLIFrameElement')) {
  failures.push('RemoteBrowser must use the Tauri-compatible iframe surface instead of Electron webview types.');
}
if (!browserSource.includes('resolveBrowserUrl?.(connectionId')) {
  failures.push('RemoteBrowser must resolve browser URLs through the Rust browser proxy bridge.');
}
if (browserSource.includes('<webview') || browserSource.includes('BrowserWebview')) {
  failures.push('RemoteBrowser must not keep Electron webview runtime code.');
}

const shellSource = normalizedText(currentPath('RemoteDesktopShell.tsx'));
if (!shellSource.includes("desktopWindow.appKey === 'monitor'")) {
  failures.push('RemoteDesktopShell must keep the explicit monitor window render branch.');
}

if (failures.length) {
  console.error(failures.join('\n\n'));
  process.exit(1);
}

console.log(`Frontend fidelity ok: ${backupFiles.length} React/TypeScript/SCSS files match backup except the documented Tauri integration files.`);

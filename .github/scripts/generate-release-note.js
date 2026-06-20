import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const artifactsDir = path.resolve(process.env.ARTIFACTS_DIR || process.argv[2] || 'artifacts');

function getVersion() {
  if (process.env.VERSION) {
    return process.env.VERSION;
  }

  const refName = process.env.GITHUB_REF_NAME;
  if (refName && /^v\d+\.\d+\.\d+/.test(refName)) {
    return refName.replace(/^v/, '');
  }

  const sha = process.env.GITHUB_SHA;
  if (sha) {
    return `0.0.0-sha-${sha.substring(0, 7)}`;
  }

  try {
    const pkgPath = path.join(__dirname, '..', '..', 'package.json');
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    return pkg.version;
  } catch {
    return '0.0.0';
  }
}

function listFiles(dir, root = dir) {
  if (!fs.existsSync(dir)) {
    return [];
  }

  return fs.readdirSync(dir, { withFileTypes: true })
    .flatMap((entry) => {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        return listFiles(fullPath, root);
      }
      if (!entry.isFile()) {
        return [];
      }
      return [path.relative(root, fullPath).replace(/\\/g, '/')];
    })
    .filter((fileName) => !/\.(sig|blockmap|json)$/i.test(fileName) && !/latest\.(ya?ml|json)$/i.test(path.basename(fileName)))
    .sort((left, right) => left.localeCompare(right));
}

function releaseAssetUrl(repo, tag, fileName) {
  return `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(path.basename(fileName))}`;
}

function platformFor(fileName) {
  const lower = fileName.toLowerCase();
  if (/\.(exe|msi|zip)$/i.test(fileName) || lower.includes('windows') || lower.includes('win32')) {
    return 'Windows';
  }
  if (/\.(dmg|app\.tar\.gz)$/i.test(fileName) || lower.includes('darwin') || lower.includes('macos')) {
    return 'macOS';
  }
  if (/\.(appimage|appimage\.tar\.gz|deb|rpm|pkg\.tar\.zst)$/i.test(fileName) || lower.includes('linux')) {
    return 'Linux';
  }
  return 'Other';
}

function labelFor(fileName) {
  const baseName = path.basename(fileName);
  const lower = baseName.toLowerCase();
  const arch = lower.includes('aarch64') || lower.includes('arm64')
    ? 'arm64'
    : lower.includes('x64') || lower.includes('x86_64') || lower.includes('amd64')
      ? 'x64'
      : '';
  const kind = lower.endsWith('.exe')
    ? 'NSIS'
    : lower.endsWith('.msi')
      ? 'MSI'
      : lower.endsWith('-portable.zip')
        ? 'Portable zip'
        : lower.endsWith('.zip')
          ? 'Zip'
          : lower.endsWith('.dmg')
            ? 'DMG'
            : lower.endsWith('.app.tar.gz')
              ? 'App tarball'
              : lower.endsWith('.appimage') || lower.endsWith('.appimage.tar.gz')
                ? 'AppImage'
                : lower.endsWith('.deb')
                  ? 'Deb'
                  : lower.endsWith('.rpm')
                    ? 'RPM'
                    : lower.endsWith('.pkg.tar.zst')
                      ? 'Pacman'
                      : 'Download';
  return [kind, arch].filter(Boolean).join(' ');
}

function groupedDownloads(files, repo, tag) {
  const groups = new Map([
    ['Windows', []],
    ['macOS', []],
    ['Linux', []],
    ['Other', []],
  ]);

  for (const fileName of files) {
    const platform = platformFor(fileName);
    const label = labelFor(fileName);
    const link = `[${label}](${releaseAssetUrl(repo, tag, fileName)})`;
    groups.get(platform)?.push(link);
  }

  return [...groups.entries()].filter(([, links]) => links.length > 0);
}

const version = getVersion();
const repo = process.env.GITHUB_REPOSITORY || 'liubaicai/ShellDesk';
const refName = process.env.GITHUB_REF_NAME;
const tag = refName && /^v\d+\.\d+\.\d+/.test(refName) ? refName : `v${version}`;
const files = listFiles(artifactsDir);
const groups = groupedDownloads(files, repo, tag);
const rows = groups.length
  ? groups.map(([platform, links]) => `| **${platform}** | ${links.join(' ')} |`).join('\n')
  : '| **All platforms** | Release assets will be attached after the package build completes. |';

const content = `## Download

| OS | Download |
| :--- | :--- |
${rows}
`;

fs.writeFileSync('release_notes.md', content);
console.log('Generated release_notes.md');

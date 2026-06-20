import fs from 'node:fs';
import path from 'node:path';

const artifactsDir = path.resolve(process.argv[2] || 'artifacts');
const outputPath = path.resolve(process.argv[3] || path.join(artifactsDir, 'latest.json'));

function readPackageVersion() {
  const pkg = JSON.parse(fs.readFileSync(path.resolve('package.json'), 'utf8'));
  return pkg.version;
}

function releaseVersion() {
  const refName = process.env.GITHUB_REF_NAME || '';
  if (/^v\d+\.\d+\.\d+/.test(refName)) {
    return refName.replace(/^v/, '');
  }
  return process.env.VERSION || readPackageVersion();
}

function releaseTag(version) {
  const refName = process.env.GITHUB_REF_NAME || '';
  return /^v\d+\.\d+\.\d+/.test(refName) ? refName : `v${version}`;
}

function releaseNotes() {
  const notesPath = process.env.RELEASE_NOTES_PATH || 'release_notes.md';
  if (!fs.existsSync(notesPath)) {
    return '';
  }
  return fs.readFileSync(notesPath, 'utf8').trim();
}

function listFiles(dir, root = dir) {
  if (!fs.existsSync(dir)) {
    throw new Error(`Artifacts directory does not exist: ${dir}`);
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
    .sort((left, right) => left.localeCompare(right));
}

function githubReleaseAssetUrl(repo, tag, fileName) {
  const assetName = path.basename(fileName);
  return `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(assetName)}`;
}

function signatureFor(fileName) {
  const signaturePath = path.join(artifactsDir, `${fileName}.sig`);
  if (!fs.existsSync(signaturePath)) {
    return '';
  }
  return fs.readFileSync(signaturePath, 'utf8').trim();
}

function includesAny(value, tokens) {
  const lower = value.toLowerCase();
  return tokens.some((token) => lower.includes(token));
}

function selectArtifact(files, candidates) {
  for (const candidate of candidates) {
    const match = files.find(candidate);
    if (!match) {
      continue;
    }
    const signature = signatureFor(match);
    if (signature) {
      return { fileName: match, signature };
    }
  }
  return null;
}

const version = releaseVersion();
const tag = releaseTag(version);
const repo = process.env.GITHUB_REPOSITORY || 'liubaicai/ShellDesk';
const files = listFiles(artifactsDir);

const windowsSetupExe = (name) => /(?:^|[-_])setup\.exe$/i.test(path.basename(name));
const windowsX64Exe = (name) => /\.exe$/i.test(name) && includesAny(name, ['x64', 'x86_64', 'amd64']);
const windowsX64SetupExe = (name) => windowsSetupExe(name) && windowsX64Exe(name);
const windowsX64Msi = (name) => /\.msi$/i.test(name) && includesAny(name, ['x64', 'x86_64', 'amd64']);
const windowsArm64Exe = (name) => /\.exe$/i.test(name) && includesAny(name, ['arm64', 'aarch64']);
const windowsArm64SetupExe = (name) => windowsSetupExe(name) && windowsArm64Exe(name);
const windowsArm64Msi = (name) => /\.msi$/i.test(name) && includesAny(name, ['arm64', 'aarch64']);
const darwinX64 = (name) => name.endsWith('.app.tar.gz') && !includesAny(name, ['aarch64', 'arm64']);
const darwinArm64 = (name) => name.endsWith('.app.tar.gz') && includesAny(name, ['aarch64', 'arm64']);
const linuxX64AppImage = (name) => name.endsWith('.AppImage') && !includesAny(name, ['aarch64', 'arm64']);
const linuxArm64AppImage = (name) => name.endsWith('.AppImage') && includesAny(name, ['aarch64', 'arm64']);
const linuxX64Archive = (name) => name.endsWith('.AppImage.tar.gz') && !includesAny(name, ['aarch64', 'arm64']);
const linuxArm64Archive = (name) => name.endsWith('.AppImage.tar.gz') && includesAny(name, ['aarch64', 'arm64']);

const platformCandidates = {
  'windows-x86_64': [
    windowsX64SetupExe,
    windowsX64Exe,
    windowsX64Msi,
    (name) => windowsSetupExe(name) && !includesAny(name, ['arm64', 'aarch64', 'i686', 'ia32']),
    (name) => /\.exe$/i.test(name) && !includesAny(name, ['arm64', 'aarch64', 'i686', 'ia32']),
    (name) => /\.msi$/i.test(name) && !includesAny(name, ['arm64', 'aarch64', 'i686', 'ia32']),
  ],
  'windows-aarch64': [windowsArm64SetupExe, windowsArm64Exe, windowsArm64Msi],
  'darwin-x86_64': [darwinX64],
  'darwin-aarch64': [darwinArm64],
  'linux-x86_64': [linuxX64AppImage, linuxX64Archive],
  'linux-aarch64': [linuxArm64AppImage, linuxArm64Archive],
};

const platforms = {};
for (const [platform, candidates] of Object.entries(platformCandidates)) {
  const artifact = selectArtifact(files, candidates);
  if (!artifact) {
    continue;
  }
  platforms[platform] = {
    signature: artifact.signature,
    url: githubReleaseAssetUrl(repo, tag, artifact.fileName),
  };
}

if (!Object.keys(platforms).length) {
  throw new Error([
    'No signed updater artifacts were found.',
    'Build with Tauri updater signing enabled and upload installer files with matching .sig files.',
  ].join(' '));
}

const manifest = {
  version,
  notes: releaseNotes(),
  pub_date: process.env.RELEASE_PUBLISHED_AT || new Date().toISOString(),
  platforms,
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Generated ${outputPath}`);

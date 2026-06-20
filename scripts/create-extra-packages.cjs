const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const repoRoot = path.resolve(__dirname, '..');
const args = process.argv.slice(2);
const packageJson = JSON.parse(fs.readFileSync(path.join(repoRoot, 'package.json'), 'utf8'));
const cargoToml = fs.readFileSync(path.join(repoRoot, 'src-tauri', 'Cargo.toml'), 'utf8');
const cargoName = cargoToml.match(/^name\s*=\s*"([^"]+)"/m)?.[1] || packageJson.name;
const productName = packageJson.productName || packageJson.name;

function argValue(name) {
  const index = args.indexOf(name);
  if (index === -1 || index === args.length - 1) {
    return '';
  }
  return args[index + 1];
}

function targetTriple() {
  return argValue('--target') || argValue('-t');
}

function releaseDirectories() {
  const target = targetTriple();
  return [
    target ? path.join(repoRoot, 'src-tauri', 'target', target, 'release') : '',
    path.join(repoRoot, 'src-tauri', 'target', 'release'),
  ].filter(Boolean);
}

function findReleaseBinary(extension) {
  const binaryName = `${cargoName}${extension}`;
  for (const directory of releaseDirectories()) {
    const binaryPath = path.join(directory, binaryName);
    if (fs.existsSync(binaryPath)) {
      return binaryPath;
    }
  }
  throw new Error(`Release binary was not found: ${binaryName}`);
}

function bundleDirectory(binaryPath, name) {
  const directory = path.join(path.dirname(binaryPath), 'bundle', name);
  fs.mkdirSync(directory, { recursive: true });
  return directory;
}

function cleanDirectory(directory) {
  fs.rmSync(directory, { recursive: true, force: true });
  fs.mkdirSync(directory, { recursive: true });
}

function copyFile(source, destination, mode) {
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.copyFileSync(source, destination);
  if (mode !== undefined) {
    fs.chmodSync(destination, mode);
  }
}

function listFiles(directory) {
  if (!fs.existsSync(directory)) {
    return [];
  }
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      return listFiles(fullPath);
    }
    return entry.isFile() ? [fullPath] : [];
  });
}

function directorySize(directory) {
  return listFiles(directory)
    .reduce((size, filePath) => size + fs.statSync(filePath).size, 0);
}

function versionForFileName() {
  return packageJson.version.replace(/[^0-9A-Za-z.+_-]/g, '_');
}

function archPackageVersion() {
  return packageJson.version.replace(/[-\s]/g, '_');
}

function archName() {
  const target = targetTriple();
  if (target.includes('aarch64')) {
    return 'aarch64';
  }
  if (target.includes('armv7')) {
    return 'armv7h';
  }
  return 'x86_64';
}

function windowsArchName() {
  const target = targetTriple();
  if (target.includes('aarch64') || target.includes('arm64')) {
    return 'arm64';
  }
  if (target.includes('i686') || target.includes('ia32')) {
    return 'ia32';
  }
  return 'x64';
}

function createWindowsPortableZip() {
  if (process.platform !== 'win32') {
    return null;
  }

  const binaryPath = findReleaseBinary('.exe');
  const outputDir = bundleDirectory(binaryPath, 'portable');
  const outputPath = path.join(outputDir, `${productName}_${versionForFileName()}_${windowsArchName()}-portable.zip`);
  const stageRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'shelldesk-portable-'));
  const appDir = path.join(stageRoot, productName);

  try {
    cleanDirectory(appDir);
    copyFile(binaryPath, path.join(appDir, `${productName}.exe`));
    fs.writeFileSync(
      path.join(appDir, 'README.txt'),
      [
        `${productName} portable package`,
        '',
        `Run ${productName}.exe directly. No installer is required.`,
        'Settings and vault data are still stored in the normal application data directory.',
        '',
      ].join('\r\n'),
    );
    fs.rmSync(outputPath, { force: true });

    const result = spawnSync('powershell.exe', [
      '-NoLogo',
      '-NoProfile',
      '-Command',
      '$ErrorActionPreference = "Stop"; Compress-Archive -Path (Join-Path $env:SHELLDESK_PORTABLE_STAGE "*") -DestinationPath $env:SHELLDESK_PORTABLE_OUTPUT -Force',
    ], {
      env: {
        ...process.env,
        SHELLDESK_PORTABLE_STAGE: appDir,
        SHELLDESK_PORTABLE_OUTPUT: outputPath,
      },
      stdio: 'inherit',
    });

    if (result.error) {
      throw result.error;
    }
    if (result.status !== 0) {
      throw new Error(`Portable zip packaging failed with exit code ${result.status}.`);
    }
    return outputPath;
  } finally {
    fs.rmSync(stageRoot, { recursive: true, force: true });
  }
}

function createPacmanPackage() {
  if (process.platform !== 'linux') {
    return null;
  }

  const binaryPath = findReleaseBinary('');
  const arch = archName();
  const pkgver = archPackageVersion();
  const outputDir = bundleDirectory(binaryPath, 'pacman');
  const outputPath = path.join(outputDir, `${cargoName}-${pkgver}-1-${arch}.pkg.tar.zst`);
  const stageRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'shelldesk-pacman-'));
  const pkgRoot = path.join(stageRoot, 'pkg');

  try {
    cleanDirectory(pkgRoot);
    copyFile(binaryPath, path.join(pkgRoot, 'usr', 'bin', cargoName), 0o755);
    copyFile(
      path.join(repoRoot, 'src-tauri', 'icons', 'icon.png'),
      path.join(pkgRoot, 'usr', 'share', 'icons', 'hicolor', '256x256', 'apps', `${cargoName}.png`),
      0o644,
    );
    fs.writeFileSync(
      path.join(pkgRoot, 'usr', 'share', 'applications', `${cargoName}.desktop`),
      [
        '[Desktop Entry]',
        'Type=Application',
        `Name=${productName}`,
        `Comment=${packageJson.description || productName}`,
        `Exec=${cargoName}`,
        `Icon=${cargoName}`,
        'Terminal=false',
        'Categories=Development;Network;TerminalEmulator;',
        '',
      ].join('\n'),
      { mode: 0o644 },
    );

    const installedSize = directorySize(pkgRoot);
    fs.writeFileSync(
      path.join(pkgRoot, '.PKGINFO'),
      [
        `pkgname = ${cargoName}`,
        `pkgbase = ${cargoName}`,
        `pkgver = ${pkgver}-1`,
        `pkgdesc = ${packageJson.description || productName}`,
        `url = ${packageJson.homepage || ''}`,
        `builddate = ${Math.floor(Date.now() / 1000)}`,
        `packager = ${packageJson.author || 'ShellDesk CI'}`,
        `size = ${installedSize}`,
        `arch = ${arch}`,
        `license = ${packageJson.license || 'custom'}`,
        'depend = webkit2gtk-4.1',
        'depend = gtk3',
        'depend = libayatana-appindicator',
        'depend = librsvg',
        '',
      ].join('\n'),
      { mode: 0o644 },
    );

    fs.rmSync(outputPath, { force: true });
    const result = spawnSync('bsdtar', [
      '--uid', '0',
      '--gid', '0',
      '--uname', 'root',
      '--gname', 'root',
      '--zstd',
      '-cf',
      outputPath,
      '.PKGINFO',
      'usr',
    ], {
      cwd: pkgRoot,
      stdio: 'inherit',
    });

    if (result.error) {
      throw result.error;
    }
    if (result.status !== 0) {
      throw new Error(`Pacman packaging failed with exit code ${result.status}.`);
    }
    return outputPath;
  } finally {
    fs.rmSync(stageRoot, { recursive: true, force: true });
  }
}

const created = [
  createWindowsPortableZip(),
  createPacmanPackage(),
].filter(Boolean);

if (created.length) {
  console.log('Created extra package artifact(s):');
  for (const artifact of created) {
    console.log(`- ${path.relative(repoRoot, artifact)}`);
  }
} else {
  console.log('No extra package artifacts are required for this platform.');
}

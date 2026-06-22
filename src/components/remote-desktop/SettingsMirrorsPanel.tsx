import { useCallback, useEffect, useState } from 'react';
import { getErrorMessage } from './desktopUtils';
import DismissibleAlert from './DismissibleAlert';
import { getCurrentAppLanguage, t, useCurrentAppLanguage, type AppLanguage, type MessageId } from '../../i18n';
import type { AptMirrorFlavor, AptSourceTarget, MirrorDistroType, SettingsConfirmDialogConfig } from './settingsTypes';
import { parseAptSourceInspection, parseKeyValueOutput, parseYumRepoInspection } from './settingsParsers';
import { isSafeHostname, SettingsCommandPreview, SettingsConfirmDialog, shellQuote, useRemoteSettingsCommand, withLinuxPrivilege } from './settingsShared';
const MIRROR_PRESETS = {
  ubuntu: [
    { labelId: 'remoteSettings.mirrors.aliyun', url: 'mirrors.aliyun.com' },
    { labelId: 'remoteSettings.mirrors.tuna', url: 'mirrors.tuna.tsinghua.edu.cn' },
    { labelId: 'remoteSettings.mirrors.ustc', url: 'mirrors.ustc.edu.cn' },
    { labelId: 'remoteSettings.mirrors.huawei', url: 'mirrors.huaweicloud.com' },
    { labelId: 'remoteSettings.mirrors.official', url: 'archive.ubuntu.com' },
  ],
  debian: [
    { labelId: 'remoteSettings.mirrors.aliyun', url: 'mirrors.aliyun.com' },
    { labelId: 'remoteSettings.mirrors.tuna', url: 'mirrors.tuna.tsinghua.edu.cn' },
    { labelId: 'remoteSettings.mirrors.ustc', url: 'mirrors.ustc.edu.cn' },
    { labelId: 'remoteSettings.mirrors.huawei', url: 'mirrors.huaweicloud.com' },
    { labelId: 'remoteSettings.mirrors.official', url: 'deb.debian.org' },
  ],
  redhat: [
    { labelId: 'remoteSettings.mirrors.aliyun', url: 'mirrors.aliyun.com' },
    { labelId: 'remoteSettings.mirrors.tuna', url: 'mirrors.tuna.tsinghua.edu.cn' },
    { labelId: 'remoteSettings.mirrors.ustc', url: 'mirrors.ustc.edu.cn' },
    { labelId: 'remoteSettings.mirrors.huawei', url: 'mirrors.huaweicloud.com' },
  ],
} as const satisfies Record<AptMirrorFlavor | 'redhat', ReadonlyArray<{ labelId: MessageId; url: string }>>;
const APT_SOURCE_CONTENT_MARKER = 'SHELLDESK_APT_SOURCE_CONTENT';
const LEGACY_APT_SOURCE_PATH = '/etc/apt/sources.list';
const UBUNTU_DEB822_SOURCE_PATH = '/etc/apt/sources.list.d/ubuntu.sources';
const DEBIAN_DEB822_SOURCE_PATH = '/etc/apt/sources.list.d/debian.sources';
const YUM_REPO_CONTENT_MARKER = 'SHELLDESK_YUM_REPO_CONTENT';
function createAptSourceInspectionCommand() {
  return [
    'if [ -f /etc/os-release ]; then',
    '  . /etc/os-release',
    'fi',
    'apt_source_path=',
    'apt_source_format=legacy',
    `if [ "\${ID:-}" = "ubuntu" ] && [ -f ${UBUNTU_DEB822_SOURCE_PATH} ]; then`,
    `  apt_source_path=${UBUNTU_DEB822_SOURCE_PATH}`,
    '  apt_source_format=deb822',
    `elif [ "\${ID:-}" = "debian" ] && [ -f ${DEBIAN_DEB822_SOURCE_PATH} ]; then`,
    `  apt_source_path=${DEBIAN_DEB822_SOURCE_PATH}`,
    '  apt_source_format=deb822',
    `elif [ -f ${UBUNTU_DEB822_SOURCE_PATH} ]; then`,
    `  apt_source_path=${UBUNTU_DEB822_SOURCE_PATH}`,
    '  apt_source_format=deb822',
    `elif [ -f ${DEBIAN_DEB822_SOURCE_PATH} ]; then`,
    `  apt_source_path=${DEBIAN_DEB822_SOURCE_PATH}`,
    '  apt_source_format=deb822',
    'elif [ -f /etc/apt/sources.list ]; then',
    '  apt_source_path=/etc/apt/sources.list',
    '  apt_source_format=legacy',
    'elif [ "${ID:-}" = "ubuntu" ]; then',
    `  apt_source_path=${UBUNTU_DEB822_SOURCE_PATH}`,
    '  apt_source_format=deb822',
    'else',
    '  apt_source_path=/etc/apt/sources.list',
    '  apt_source_format=legacy',
    'fi',
    'printf "APT_SOURCE_PATH=%s\\n" "$apt_source_path"',
    'printf "APT_SOURCE_FORMAT=%s\\n" "$apt_source_format"',
    `printf '%s\\n' '${APT_SOURCE_CONTENT_MARKER}'`,
    'if [ -n "$apt_source_path" ] && [ -f "$apt_source_path" ]; then',
    '  sed -n "1,160p" "$apt_source_path" 2>/dev/null',
    'fi',
  ].join('\n');
}
function getAptFlavorFromDistroOutput(output: string): AptMirrorFlavor {
  const values = parseKeyValueOutput(output);
  const id = (values.get('ID') ?? '').toLowerCase();
  const idLike = (values.get('ID_LIKE') ?? '').toLowerCase();
  if (['ubuntu', 'linuxmint', 'pop', 'elementary'].includes(id) || /(^|[\s,])ubuntu(?=$|[\s,])/.test(idLike)) {
    return 'ubuntu';
  }
  return 'debian';
}
function getDefaultAptSourceTarget(flavor: AptMirrorFlavor): AptSourceTarget {
  if (flavor === 'ubuntu') {
    return { path: UBUNTU_DEB822_SOURCE_PATH, format: 'deb822', flavor };
  }
  return { path: LEGACY_APT_SOURCE_PATH, format: 'legacy', flavor };
}
function createYumRepoInspectionCommand() {
  return [
    'repo_dir=/etc/yum.repos.d',
    'printf "YUM_REPO_DIR=%s\\n" "$repo_dir"',
    `printf '%s\\n' '${YUM_REPO_CONTENT_MARKER}'`,
    'found=0',
    'if ls "$repo_dir"/*.repo >/dev/null 2>&1; then',
    '  for repo_file in "$repo_dir"/*.repo; do',
    '    [ -f "$repo_file" ] || continue',
    '    found=1',
    '    printf "\\n# %s\\n" "$repo_file"',
    `    awk '
      /^[[:space:]]*\\[[^]]+\\][[:space:]]*$/ { print; next }
      /^[[:space:]]*#?[[:space:]]*(name|baseurl|mirrorlist|metalink|enabled)[[:space:]]*=/ { print; next }
    ' "$repo_file"`,
    '  done',
    'fi',
    'if [ "$found" -eq 0 ]; then',
    '  printf "No repo files found under %s\\n" "$repo_dir"',
    'fi',
  ].join('\n');
}
function isOfficialRhelDistro(values: Map<string, string>) {
  const id = (values.get('ID') ?? '').toLowerCase();
  const name = (values.get('NAME') ?? '').toLowerCase();
  return id === 'rhel' || id === 'redhat' || /\bred hat enterprise linux\b/.test(name);
}
function normalizeAptCodename(rawCodename: string | undefined, flavor: AptMirrorFlavor) {
  const fallback = flavor === 'ubuntu' ? 'noble' : 'bookworm';
  const codename = (rawCodename ?? '').trim();
  return /^[A-Za-z0-9._-]+$/.test(codename) ? codename : fallback;
}
function buildUbuntuLegacySources(mirrorUrl: string, codename: string) {
  const components = 'main restricted universe multiverse';
  const archiveUri = `http://${mirrorUrl}/ubuntu/`;
  const securityUri = mirrorUrl === 'archive.ubuntu.com' ? 'http://security.ubuntu.com/ubuntu/' : archiveUri;
  return [
    `deb ${archiveUri} ${codename} ${components}`,
    `deb ${archiveUri} ${codename}-updates ${components}`,
    `deb ${archiveUri} ${codename}-backports ${components}`,
    `deb ${securityUri} ${codename}-security ${components}`,
  ].join('\n');
}
function buildDebianLegacySources(mirrorUrl: string, codename: string) {
  const components = 'main contrib non-free non-free-firmware';
  return [
    `deb http://${mirrorUrl}/debian/ ${codename} ${components}`,
    `deb http://${mirrorUrl}/debian/ ${codename}-updates ${components}`,
    `deb http://${mirrorUrl}/debian/ ${codename}-backports ${components}`,
    `deb http://${mirrorUrl}/debian-security ${codename}-security ${components}`,
  ].join('\n');
}
function buildUbuntuDeb822Sources(mirrorUrl: string, codename: string) {
  const components = 'main restricted universe multiverse';
  const signedBy = '/usr/share/keyrings/ubuntu-archive-keyring.gpg';
  const archiveUri = `http://${mirrorUrl}/ubuntu/`;
  const securityUri = mirrorUrl === 'archive.ubuntu.com' ? 'http://security.ubuntu.com/ubuntu/' : archiveUri;
  if (securityUri === archiveUri) {
    return [
      'Types: deb',
      `URIs: ${archiveUri}`,
      `Suites: ${codename} ${codename}-updates ${codename}-backports ${codename}-security`,
      `Components: ${components}`,
      `Signed-By: ${signedBy}`,
    ].join('\n');
  }
  return [
    [
      'Types: deb',
      `URIs: ${archiveUri}`,
      `Suites: ${codename} ${codename}-updates ${codename}-backports`,
      `Components: ${components}`,
      `Signed-By: ${signedBy}`,
    ].join('\n'),
    [
      'Types: deb',
      `URIs: ${securityUri}`,
      `Suites: ${codename}-security`,
      `Components: ${components}`,
      `Signed-By: ${signedBy}`,
    ].join('\n'),
  ].join('\n\n');
}
function buildDebianDeb822Sources(mirrorUrl: string, codename: string) {
  const components = 'main contrib non-free non-free-firmware';
  const signedBy = '/usr/share/keyrings/debian-archive-keyring.gpg';
  return [
    [
      'Types: deb',
      `URIs: http://${mirrorUrl}/debian/`,
      `Suites: ${codename} ${codename}-updates ${codename}-backports`,
      `Components: ${components}`,
      `Signed-By: ${signedBy}`,
    ].join('\n'),
    [
      'Types: deb',
      `URIs: http://${mirrorUrl}/debian-security`,
      `Suites: ${codename}-security`,
      `Components: ${components}`,
      `Signed-By: ${signedBy}`,
    ].join('\n'),
  ].join('\n\n');
}
function buildAptSourcesContent(mirrorUrl: string, target: AptSourceTarget, codename: string) {
  if (target.format === 'deb822') {
    return target.flavor === 'ubuntu'
      ? buildUbuntuDeb822Sources(mirrorUrl, codename)
      : buildDebianDeb822Sources(mirrorUrl, codename);
  }
  return target.flavor === 'ubuntu'
    ? buildUbuntuLegacySources(mirrorUrl, codename)
    : buildDebianLegacySources(mirrorUrl, codename);
}
export default function SettingsMirrorsPanel() {
  const language = useCurrentAppLanguage();
  const runCommand = useRemoteSettingsCommand();
  const [distroType, setDistroType] = useState<MirrorDistroType>('unknown');
  const [distroName, setDistroName] = useState('');
  const [aptSourceTarget, setAptSourceTarget] = useState<AptSourceTarget | null>(null);
  const [currentMirror, setCurrentMirror] = useState('');
  const [mirrorDraft, setMirrorDraft] = useState('');
  const [loading, setLoading] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');
  const [confirmDialog, setConfirmDialog] = useState<SettingsConfirmDialogConfig | null>(null);
  const detectDistro = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const detectResult = await runCommand(`
        if [ -f /etc/os-release ]; then
          . /etc/os-release
          echo "ID=$ID"
          echo "ID_LIKE=$ID_LIKE"
          echo "VERSION_CODENAME=$VERSION_CODENAME"
          echo "NAME=$NAME"
        elif command -v apt-get >/dev/null 2>&1; then
          echo "TYPE=debian"
        elif command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1; then
          echo "TYPE=redhat"
        else
          echo "TYPE=unknown"
        fi
      `);
      const output = detectResult.stdout;
      const distroValues = parseKeyValueOutput(output);
      const distroId = (distroValues.get('ID') ?? '').toLowerCase();
      const distroLike = (distroValues.get('ID_LIKE') ?? '').toLowerCase();
      setDistroName(output);
      if (
        ['ubuntu', 'debian', 'kali', 'linuxmint', 'pop', 'elementary', 'raspbian'].includes(distroId)
        || /(^|[\s,])(ubuntu|debian)(?=$|[\s,])/.test(distroLike)
        || /TYPE=debian/i.test(output)
      ) {
        setDistroType('debian');
        const flavor = getAptFlavorFromDistroOutput(output);
        const mirrorResult = await runCommand(createAptSourceInspectionCommand());
        const sourceInspection = parseAptSourceInspection(mirrorResult.stdout, flavor, language);
        setAptSourceTarget(sourceInspection.target);
        setCurrentMirror(sourceInspection.display);
      } else if (
        ['centos', 'rhel', 'fedora', 'rocky', 'alma', 'almalinux', 'ol', 'amzn'].includes(distroId)
        || /(^|[\s,])(rhel|fedora|centos)(?=$|[\s,])/.test(distroLike)
      ) {
        setDistroType(isOfficialRhelDistro(distroValues) ? 'rhel' : 'redhat');
        setAptSourceTarget(null);
        const mirrorResult = await runCommand(createYumRepoInspectionCommand());
        setCurrentMirror(parseYumRepoInspection(mirrorResult.stdout || mirrorResult.stderr || '', language));
      } else {
        setDistroType('unknown');
        setAptSourceTarget(null);
        setCurrentMirror('');
      }
      setMirrorDraft('');
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [language, runCommand]);
  useEffect(() => { void detectDistro(); }, [detectDistro]);
  const getMirrorPlan = (mirrorUrl: string) => {
    if (!isSafeHostname(mirrorUrl)) {
      throw new Error(t('remoteSettings.mirrors.invalidDomain', language));
    }
    if (distroType === 'debian') {
      const versionMatch = distroName.match(/VERSION_CODENAME=(\S+)/);
      const flavor = aptSourceTarget?.flavor ?? getAptFlavorFromDistroOutput(distroName);
      const target = aptSourceTarget ?? getDefaultAptSourceTarget(flavor);
      const codename = normalizeAptCodename(versionMatch?.[1], flavor);
      const pathArg = shellQuote(target.path);
      const content = buildAptSourcesContent(mirrorUrl, target, codename);
      const backupCommand = `if [ -f ${pathArg} ]; then cp ${pathArg} ${pathArg}.bak.$(date +%s); fi`;
      const prepareCommand = target.format === 'deb822' ? 'mkdir -p /etc/apt/sources.list.d' : '';
      const writeCommand = `${prepareCommand ? `${prepareCommand}\n` : ''}cat > ${pathArg} << 'MIRROR_EOF'
${content}
MIRROR_EOF`;
      return {
        backupCommand,
        writeCommand,
        preview: `${backupCommand}\n${writeCommand}`,
        successMessage: t('remoteSettings.mirrors.switchAptSuccess', language, { path: target.path, mirror: mirrorUrl }),
      };
    }
    if (distroType === 'redhat') {
      const backupCommand = `cp -r /etc/yum.repos.d /etc/yum.repos.d.bak.$(date +%s) 2>/dev/null`;
      const writeCommand = `sed -i 's|^mirrorlist=|#mirrorlist=|g; s|^#\\(baseurl=.*\\)baseurl|\\1baseurl|g; s|baseurl=.*://[^/]*|baseurl=http://${mirrorUrl}|g' /etc/yum.repos.d/*.repo 2>/dev/null`;
      return {
        backupCommand,
        writeCommand,
        preview: `${backupCommand}\n${writeCommand}`,
        successMessage: t('remoteSettings.mirrors.switchYumSuccess', language, { mirror: mirrorUrl }),
      };
    }
    throw new Error(t('remoteSettings.mirrors.unknownDistro', language));
  };
  const applyMirror = async (mirrorUrl: string) => {
    setApplying(true);
    setError('');
    setSuccess('');
    try {
      const plan = getMirrorPlan(mirrorUrl);
      const backupResult = await runCommand(withLinuxPrivilege(plan.backupCommand));
      const writeResult = await runCommand(withLinuxPrivilege(plan.writeCommand));
      if (writeResult.code !== 0) {
        throw new Error(writeResult.stderr || writeResult.stdout || t('remoteSettings.mirrors.writeFailed', language));
      }
      setSuccess(backupResult.code === 0 ? plan.successMessage : t('remoteSettings.mirrors.backupMaybeFailed', language, { message: plan.successMessage }));
      setMirrorDraft('');
      await detectDistro();
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setApplying(false);
    }
  };
  const requestApplyMirror = () => {
    if (!mirrorDraft) return;
    try {
      const plan = getMirrorPlan(mirrorDraft);
      setConfirmDialog({
        title: t('remoteSettings.mirrors.switchTitle', language),
        message: t('remoteSettings.mirrors.switchMessage', language, { mirror: mirrorDraft }),
        detail: t('remoteSettings.mirrors.switchDetail', language),
        preview: plan.preview,
        confirmLabel: t('remoteSettings.mirrors.switchConfirm', language),
        tone: 'warning',
        onConfirm: () => applyMirror(mirrorDraft),
      });
    } catch (err) {
      setError(getErrorMessage(err));
    }
  };
  const aptFlavor = aptSourceTarget?.flavor ?? getAptFlavorFromDistroOutput(distroName);
  const presets = distroType === 'debian'
    ? MIRROR_PRESETS[aptFlavor]
    : distroType === 'redhat'
      ? MIRROR_PRESETS.redhat
      : [];
  const canQuickSwitchMirror = distroType !== 'rhel';
  return (
    <div className="settings-panel-content">
      <div className="settings-panel-header">
        <div>
          <h3>{t('remoteSettings.mirrors.title', language)}</h3>
          <p>{t('remoteSettings.mirrors.description', language)}</p>
        </div>
        <button type="button" className="settings-action-btn" onClick={detectDistro} disabled={loading}>
          {loading ? t('remoteSettings.mirrors.detecting', language) : t('remoteSettings.mirrors.redetect', language)}
        </button>
      </div>
      {error ? (
        <DismissibleAlert className="error-banner" source="RemoteSettings" onDismiss={() => setError('')} role="alert">
          {error}
        </DismissibleAlert>
      ) : null}
      {success ? (
        <DismissibleAlert className="settings-success-banner" onDismiss={() => setSuccess('')}>
          {success}
        </DismissibleAlert>
      ) : null}
      <div className="settings-info-card">
        <span className="settings-info-label">{t('remoteSettings.mirrors.distro', language)}</span>
        <strong className="settings-info-value">{distroName.split('\n').filter(l => l.startsWith('NAME=')).map(l => l.replace('NAME=', '')).join('') || t('remoteSettings.mirrors.detecting', language)}</strong>
      </div>
      {distroType !== 'unknown' ? (
        <>
          <div className="settings-section">
            <h4>{t('remoteSettings.mirrors.current', language)}</h4>
            <pre className="settings-output">{currentMirror || t('remoteSettings.common.loading', language)}</pre>
          </div>
          {canQuickSwitchMirror ? (
            <div className="settings-section">
              <h4>{t('remoteSettings.mirrors.quickSwitch', language)}</h4>
              <div className="settings-mirror-grid">
                {presets.map((preset) => (
                  <button
                    key={preset.url}
                    type="button"
                    className={`settings-mirror-btn ${mirrorDraft === preset.url ? 'selected' : ''}`}
                    onClick={() => { setMirrorDraft(preset.url); setSuccess(''); setError(''); }}
                    disabled={applying}
                  >
                    <strong>{t(preset.labelId, language)}</strong>
                    <small>{preset.url}</small>
                  </button>
                ))}
              </div>
              {mirrorDraft ? (
                <div className="settings-preview-card">
                  <div>
                    <strong>{t('remoteSettings.mirrors.pending', language)}</strong>
                    <span>{mirrorDraft}</span>
                  </div>
                  <SettingsCommandPreview label={t('remoteSettings.mirrors.commandPreview', language)} content={getMirrorPlan(mirrorDraft).preview} />
                  <div className="settings-preview-actions">
                    <button type="button" className="settings-action-btn" onClick={() => setMirrorDraft('')} disabled={applying}>{t('remoteSettings.mirrors.clearDraft', language)}</button>
                    <button type="button" className="settings-action-btn primary" onClick={requestApplyMirror} disabled={applying}>
                      {applying ? t('remoteSettings.common.applyingConfig', language) : t('remoteSettings.network.previewApply', language)}
                    </button>
                  </div>
                </div>
              ) : null}
            </div>
          ) : (
            <div className="settings-section">
              <h4>{t('remoteSettings.mirrors.quickUnavailable', language)}</h4>
              <p className="settings-hint">
                {t('remoteSettings.mirrors.rhelHint', language)}
              </p>
            </div>
          )}
        </>
      ) : (
        <div className="settings-section">
          <p className="settings-hint">{t('remoteSettings.mirrors.unknownHint', language)}</p>
        </div>
      )}
      {confirmDialog ? <SettingsConfirmDialog config={confirmDialog} onClose={() => setConfirmDialog(null)} /> : null}
    </div>
  );
}

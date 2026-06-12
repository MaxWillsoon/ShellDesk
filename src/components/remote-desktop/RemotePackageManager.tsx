import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import DismissibleAlert from './DismissibleAlert';

import { getErrorMessage, getShellDeskLocale } from './desktopUtils';
import {
  createDetectPackageManagerCommand,
  createPackageActionCommand,
  createPackageListCommand,
  createPackageSearchCommand,
  createPackageSourceInspectCommand,
  createPackageSourceSaveCommand,
  getPackageManagerLabel,
  normalizePackageManager,
  parsePackageOutput,
  parsePackageSourceFiles,
  type PackageAction,
  type PackageManagerKind,
  type PackageSourceFile,
  type PackageView,
  type RemotePackageInfo,
} from './packageProviders';
import { isWindowsSystem } from './remoteSystem';
import type { RemoteTerminalLaunchOptions } from './RemoteTerminal';
import { useSudoCommand } from './sudoPrompt';
import type { RemoteSystemType } from './types';
import { tCurrent } from '../../i18n';

interface RemotePackageManagerProps {
  connectionId: string;
  systemType?: RemoteSystemType;
  onOpenTerminal?: (launchOptions?: RemoteTerminalLaunchOptions) => void;
}

interface PendingPackageAction {
  action: PackageAction | 'save-source';
  label: string;
  command: string;
  packageName?: string;
  danger?: boolean;
  preview?: string;
}

type PackageActionOutputState = 'idle' | 'running' | 'success' | 'error';
type PackageWorkspaceMode = 'packages' | 'sources';

const packageViews: Array<{ key: Exclude<PackageView, 'search'>; label: string }> = [
  { key: 'upgradable', label: tCurrent('auto.remotePackageManager.13p2xv8') },
  { key: 'installed', label: tCurrent('auto.remotePackageManager.hdlzwj') },
];

const maxPackageActionOutputLength = 30000;

function appendBoundedOutput(current: string, chunk: string) {
  if (!chunk) {
    return current;
  }

  const next = `${current}${chunk}`;

  if (next.length <= maxPackageActionOutputLength) {
    return next;
  }

  return tCurrent('auto.remotePackageManager.1lc7uz5', { value0: next.slice(-maxPackageActionOutputLength) });
}

function getPackageVersion(pkg: RemotePackageInfo) {
  if (pkg.upgradable && pkg.latestVersion) {
    return `${pkg.version ?? '-'} → ${pkg.latestVersion}`;
  }

  return pkg.version || pkg.latestVersion || '-';
}

function createSourceChangePreview(current: string, draft: string) {
  if (current === draft) {
    return tCurrent('auto.remotePackageManager.16jd8ra');
  }

  const currentLines = current.split(/\r?\n/);
  const draftLines = draft.split(/\r?\n/);
  const currentLineSet = new Set(currentLines);
  const draftLineSet = new Set(draftLines);
  const added = draftLines.filter((line) => line.trim() && !currentLineSet.has(line)).slice(0, 12);
  const removed = currentLines.filter((line) => line.trim() && !draftLineSet.has(line)).slice(0, 12);

  return [
    tCurrent('auto.remotePackageManager.1y0gzhx', { value0: currentLines.length, value1: draftLines.length }),
    '',
    tCurrent('auto.remotePackageManager.7mg4pa'),
    ...(added.length ? added.map((line) => `+ ${line}`) : [tCurrent('auto.remotePackageManager.11dg74m')]),
    '',
    tCurrent('auto.remotePackageManager.2rsx5w'),
    ...(removed.length ? removed.map((line) => `- ${line}`) : [tCurrent('auto.remotePackageManager.11dg74m')]),
  ].join('\n');
}

function RemotePackageManager({ connectionId, systemType, onOpenTerminal }: RemotePackageManagerProps) {
  const isWindowsHost = isWindowsSystem(systemType);
  const { runCommand, runCommandStream, sudoPrompt } = useSudoCommand(connectionId, systemType);
  const [managerKind, setManagerKind] = useState<PackageManagerKind>('unknown');
  const [activeView, setActiveView] = useState<PackageView>('upgradable');
  const [packages, setPackages] = useState<RemotePackageInfo[]>([]);
  const [workspaceMode, setWorkspaceMode] = useState<PackageWorkspaceMode>('packages');
  const [selectedName, setSelectedName] = useState('');
  const [searchQuery, setSearchQuery] = useState('');
  const [lastSearchQuery, setLastSearchQuery] = useState('');
  const [sourceFiles, setSourceFiles] = useState<PackageSourceFile[]>([]);
  const [selectedSourcePath, setSelectedSourcePath] = useState('');
  const [sourceDraft, setSourceDraft] = useState('');
  const [sourceLoading, setSourceLoading] = useState(false);
  const [loading, setLoading] = useState(false);
  const [actionRunning, setActionRunning] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [lastRefreshedAt, setLastRefreshedAt] = useState('');
  const [pendingAction, setPendingAction] = useState<PendingPackageAction | null>(null);
  const [actionOutput, setActionOutput] = useState('');
  const [actionOutputState, setActionOutputState] = useState<PackageActionOutputState>('idle');
  const loadRequestIdRef = useRef(0);
  const bootKeyRef = useRef('');
  const actionOutputRef = useRef<HTMLPreElement | null>(null);

  const selectedPackage = useMemo(() => {
    return packages.find((pkg) => pkg.name === selectedName) ?? packages[0] ?? null;
  }, [packages, selectedName]);
  const selectedSourceFile = useMemo(() => {
    return sourceFiles.find((file) => file.path === selectedSourcePath) ?? sourceFiles[0] ?? null;
  }, [selectedSourcePath, sourceFiles]);
  const sourceDraftDirty = Boolean(selectedSourceFile && selectedSourceFile.content !== sourceDraft);
  const selectedSourceEditable = Boolean(selectedSourceFile && (selectedSourceFile.readable || !selectedSourceFile.exists));
  const selectedPrimaryAction = useMemo(() => {
    if (!selectedPackage) {
      return null;
    }

    if (selectedPackage.upgradable) {
      return { label: tCurrent('auto.remotePackageManager.1vmp8k3'), action: 'upgrade' as PackageAction, disabled: false };
    }

    if (selectedPackage.installed) {
      return { label: tCurrent('auto.remotePackageManager.hdlzwj2'), action: null, disabled: true };
    }

    return { label: tCurrent('auto.remotePackageManager.h3munn'), action: 'install' as PackageAction, disabled: false };
  }, [selectedPackage]);

  const detectManager = useCallback(async () => {
    setLoading(true);
    setError('');

    try {
      const result = await runCommand(createDetectPackageManagerCommand(isWindowsHost));
      const kind = normalizePackageManager(result.stdout.split(/\r?\n/).find(Boolean) ?? 'unknown');
      setManagerKind(kind);
      return kind;
    } catch (error) {
      setError(getErrorMessage(error));
      return 'unknown' as PackageManagerKind;
    } finally {
      setLoading(false);
    }
  }, [isWindowsHost, runCommand]);

  const loadPackages = useCallback(async (view: PackageView, kind = managerKind, query = '') => {
    const requestId = loadRequestIdRef.current + 1;
    loadRequestIdRef.current = requestId;

    if (kind === 'unknown') {
      setPackages([]);
      setError(tCurrent('auto.remotePackageManager.2itani'));
      return;
    }

    setLoading(true);
    setError('');
    setNotice('');
    setWorkspaceMode('packages');
    setPackages([]);
    setSelectedName('');
    setActiveView(view);
    if (view === 'search') {
      setLastSearchQuery(query.trim());
    }

    try {
      const command = view === 'search'
        ? createPackageSearchCommand(kind, query)
        : createPackageListCommand(kind, view);
      const result = await runCommand(command);

      if (requestId !== loadRequestIdRef.current) {
        return;
      }

      const nextPackages = parsePackageOutput(kind, view, result.stdout);
      setPackages(nextPackages);
      setSelectedName(nextPackages[0]?.name ?? '');
      setLastRefreshedAt(new Date().toLocaleTimeString(getShellDeskLocale()));
      if (result.stderr.trim()) {
        setNotice(result.stderr.trim());
      }
    } catch (error) {
      if (requestId !== loadRequestIdRef.current) {
        return;
      }

      setError(getErrorMessage(error));
    } finally {
      if (requestId === loadRequestIdRef.current) {
        setLoading(false);
      }
    }
  }, [managerKind, runCommand]);

  const loadPackageSources = useCallback(async (kind = managerKind, preferredPath = selectedSourcePath) => {
    if (kind === 'unknown' || kind === 'winget' || kind === 'choco') {
      setError(tCurrent('auto.remotePackageManager.pfwf28'));
      setWorkspaceMode('sources');
      setSourceFiles([]);
      setSelectedSourcePath('');
      setSourceDraft('');
      return;
    }

    setWorkspaceMode('sources');
    setSourceLoading(true);
    setError('');
    setNotice('');

    try {
      const result = await runCommand(createPackageSourceInspectCommand(kind));
      const nextFiles = parsePackageSourceFiles(result.stdout);
      const nextSelected = nextFiles.find((file) => file.path === preferredPath) ?? nextFiles[0] ?? null;
      setSourceFiles(nextFiles);
      setSelectedSourcePath(nextSelected?.path ?? '');
      setSourceDraft(nextSelected?.content ?? '');
      if (result.stderr.trim()) {
        setNotice(result.stderr.trim());
      }
      if (!nextFiles.length) {
        setNotice(tCurrent('auto.remotePackageManager.1srs40g'));
      }
    } catch (error) {
      setError(getErrorMessage(error));
    } finally {
      setSourceLoading(false);
    }
  }, [managerKind, runCommand, selectedSourcePath]);

  useEffect(() => {
    const bootKey = `${connectionId}:${isWindowsHost ? 'windows' : 'unix'}`;
    if (bootKeyRef.current === bootKey) {
      return;
    }
    bootKeyRef.current = bootKey;

    const boot = async () => {
      const kind = await detectManager();
      if (kind !== 'unknown') {
        await loadPackages('upgradable', kind);
      }
    };

    void boot();
  }, [detectManager, loadPackages]);

  useEffect(() => {
    const outputElement = actionOutputRef.current;
    if (!outputElement) return;
    outputElement.scrollTop = outputElement.scrollHeight;
  }, [actionOutput]);

  const resetActionOutput = () => {
    setActionOutput('');
    setActionOutputState('idle');
  };

  const closePendingAction = () => {
    if (actionRunning) return;
    setPendingAction(null);
    resetActionOutput();
  };

  const prepareAction = (action: PackageAction, pkg?: RemotePackageInfo) => {
    try {
      const packageName = pkg?.name;
      const command = createPackageActionCommand(managerKind, action, packageName);
      const labels: Record<PackageAction, string> = {
        install: tCurrent('auto.remotePackageManager.h3munn2'),
        remove: tCurrent('auto.remotePackageManager.1lca2z2'),
        upgrade: tCurrent('auto.remotePackageManager.1vmp8k32'),
        'upgrade-all': tCurrent('auto.remotePackageManager.1cq3xbf'),
        refresh: tCurrent('auto.remotePackageManager.110bnp'),
      };

      setPendingAction({
        action,
        label: labels[action],
        command,
        packageName,
        danger: action === 'remove' || action === 'upgrade-all',
      });
      resetActionOutput();
    } catch (error) {
      setError(getErrorMessage(error));
    }
  };

  const selectSourceFile = (file: PackageSourceFile) => {
    setSelectedSourcePath(file.path);
    setSourceDraft(file.content);
    setError('');
    setNotice('');
  };

  const requestSaveSource = () => {
    if (!selectedSourceFile) return;

    if (!sourceDraftDirty) {
      setNotice(tCurrent('auto.remotePackageManager.16jd8ra'));
      return;
    }

    try {
      setPendingAction({
        action: 'save-source',
        label: tCurrent('auto.remotePackageManager.1hfqjob'),
        command: createPackageSourceSaveCommand(selectedSourceFile.path, sourceDraft),
        packageName: selectedSourceFile.path,
        danger: true,
        preview: createSourceChangePreview(selectedSourceFile.content, sourceDraft),
      });
      resetActionOutput();
    } catch (error) {
      setError(getErrorMessage(error));
    }
  };

  const copyPendingCommand = async () => {
    if (!pendingAction) return;
    await navigator.clipboard.writeText(pendingAction.command);
    setNotice(tCurrent('auto.remotePackageManager.1ys75c3'));
  };

  const runPendingInTerminal = () => {
    if (!pendingAction) return;
    onOpenTerminal?.({
      title: pendingAction.label,
      initialCommand: pendingAction.command,
    });
    closePendingAction();
  };

  const executePendingAction = async () => {
    if (!pendingAction) return;

    const outputCommandLabel = pendingAction.action === 'save-source'
      ? `${pendingAction.label} ${pendingAction.packageName ?? ''}`.trim()
      : pendingAction.command;

    setActionRunning(true);
    setActionOutput(`$ ${outputCommandLabel}\n`);
    setActionOutputState('running');
    setError('');
    setNotice('');

    try {
      let receivedChunk = false;
      const result = await runCommandStream(pendingAction.command, undefined, {
        onChunk: (chunk) => {
          receivedChunk = true;
          setActionOutput((current) => appendBoundedOutput(current, chunk));
        },
      }, {
        onSudoAttempt: () => setActionOutput(`$ ${outputCommandLabel}\n`),
      });

      if (!receivedChunk) {
        const finalOutput = [result.stdout, result.stderr].filter(Boolean).join('\n');
        if (finalOutput) {
          setActionOutput((current) => appendBoundedOutput(current, `${finalOutput}\n`));
        }
      }

      setActionOutput((current) => appendBoundedOutput(current, tCurrent('auto.remotePackageManager.1tbpxd4', { value0: result.code, value1: pendingAction.label })));
      setActionOutputState(result.code === 0 ? 'success' : 'error');

      if (result.code === 0) {
        if (pendingAction.action === 'save-source') {
          await loadPackageSources(managerKind, pendingAction.packageName);
        } else {
          await loadPackages(activeView, managerKind, activeView === 'search' ? lastSearchQuery : '');
        }
        setNotice(tCurrent('auto.remotePackageManager.nkcjw9', { value0: pendingAction.label }));
      } else {
        setError(tCurrent('auto.remotePackageManager.1fdgco1', { value0: pendingAction.label, value1: result.code }));
      }
    } catch (error) {
      setActionOutput((current) => appendBoundedOutput(current, tCurrent('auto.remotePackageManager.127m15g', { value0: getErrorMessage(error) })));
      setActionOutputState('error');
      setError(getErrorMessage(error));
    } finally {
      setActionRunning(false);
    }
  };

  return (
    <section className="package-manager">
      <header className="package-toolbar">
        <div className="package-status">
          <span>{tCurrent('auto.remotePackageManager.1sgu85')}</span>
          <strong>{getPackageManagerLabel(managerKind)}</strong>
          {lastRefreshedAt ? <em>{lastRefreshedAt}</em> : null}
        </div>
        <div className="package-toolbar-actions">
          <button type="button" onClick={detectManager} disabled={loading}>{tCurrent('auto.remotePackageManager.1ot472x')}</button>
          <button type="button" onClick={() => void loadPackageSources()} disabled={sourceLoading || managerKind === 'unknown'}>{tCurrent('auto.remotePackageManager.qvxb20')}</button>
          <button type="button" onClick={() => prepareAction('refresh')} disabled={managerKind === 'unknown'}>{tCurrent('auto.remotePackageManager.110bnp2')}</button>
          <button type="button" className="primary" onClick={() => prepareAction('upgrade-all')} disabled={managerKind === 'unknown'}>{tCurrent('auto.remotePackageManager.1cq3xbf2')}</button>
        </div>
      </header>

      {error ? <DismissibleAlert className="package-alert danger" onDismiss={() => setError('')} role="alert">{error}</DismissibleAlert> : null}
      {notice ? <DismissibleAlert className="package-alert info" onDismiss={() => setNotice('')}>{notice}</DismissibleAlert> : null}

      {workspaceMode === 'sources' ? (
        <div className="package-layout source-mode">
          <aside className="package-sidebar">
            <div className="package-nav-group">
              <span className="package-sidebar-label">{tCurrent('auto.remotePackageManager.18gkps8')}</span>
              <div className="package-source-list">
                {sourceFiles.map((file) => (
                  <button
                    key={file.path}
                    type="button"
                    className={selectedSourceFile?.path === file.path ? 'active' : ''}
                    onClick={() => selectSourceFile(file)}
                  >
                    <strong>{file.path.split('/').pop() || file.path}</strong>
                    <span>{file.path}</span>
                  </button>
                ))}
              </div>
            </div>
            <button type="button" className="package-sidebar-action" onClick={() => void loadPackageSources()} disabled={sourceLoading || managerKind === 'unknown'}>
              {sourceLoading ? tCurrent('auto.remotePackageManager.6svkbt') : tCurrent('auto.remotePackageManager.h4t3jz')}
            </button>
            <button type="button" className="package-sidebar-action" onClick={() => setWorkspaceMode('packages')}>
              {tCurrent('auto.remotePackageManager.1tdu8gu')}
            </button>
          </aside>

          <main className="package-list-panel package-source-editor-panel">
            {selectedSourceFile ? (
              <>
                <div className="package-source-editor-header">
                  <div>
                    <span>{tCurrent('auto.remotePackageManager.10y3552')}</span>
                    <strong>{selectedSourceFile.path}</strong>
                  </div>
                  <span className={`package-pill ${sourceDraftDirty ? 'warning' : 'success'}`}>
                    {sourceDraftDirty ? tCurrent('auto.remotePackageManager.1bduy4n') : tCurrent('auto.remotePackageManager.16jd8ra')}
                  </span>
                </div>
                <textarea
                  className="package-source-editor"
                  value={sourceDraft}
                  onChange={(event) => setSourceDraft(event.target.value)}
                  spellCheck={false}
                  disabled={!selectedSourceEditable}
                />
              </>
            ) : (
              <div className="package-empty detail">{sourceLoading ? tCurrent('auto.remotePackageManager.6svkbt') : tCurrent('auto.remotePackageManager.1srs40g')}</div>
            )}
          </main>

          <aside className="package-detail-panel">
            {selectedSourceFile ? (
              <>
                <div className="package-detail-title">
                  <span>{tCurrent('auto.remotePackageManager.qvxb20')}</span>
                  <strong>{selectedSourceFile.path.split('/').pop() || selectedSourceFile.path}</strong>
                </div>
                <dl>
                  <div><dt>{tCurrent('auto.remotePackageManager.10y3552')}</dt><dd>{selectedSourceFile.path}</dd></div>
                  <div><dt>{tCurrent('auto.remotePackageManager.r4v5rk')}</dt><dd>{selectedSourceFile.exists ? tCurrent('auto.remotePackageManager.n6qkzx') : tCurrent('auto.remotePackageManager.w4vz40')}</dd></div>
                  <div><dt>{tCurrent('auto.remotePackageManager.1r8s3zl')}</dt><dd>{selectedSourceFile.readable ? tCurrent('auto.remotePackageManager.n6qkzx') : tCurrent('auto.remotePackageManager.w4vz40')}</dd></div>
                </dl>
                <div className="package-detail-actions">
                  <button type="button" className="primary" onClick={requestSaveSource} disabled={!selectedSourceEditable || !sourceDraftDirty || actionRunning}>
                    {tCurrent('auto.remotePackageManager.1hfqjob')}
                  </button>
                  <button type="button" onClick={() => setSourceDraft(selectedSourceFile.content)} disabled={!sourceDraftDirty}>
                    {tCurrent('auto.remotePackageManager.1ij3f4x')}
                  </button>
                </div>
                <div className="package-note">{tCurrent('auto.remotePackageManager.6ek2rh')}</div>
              </>
            ) : (
              <div className="package-empty detail">{tCurrent('auto.remotePackageManager.1srs40g')}</div>
            )}
          </aside>
        </div>
      ) : (
        <div className="package-layout">
          <aside className="package-sidebar">
            <div className="package-nav-group">
              <span className="package-sidebar-label">{tCurrent('auto.remotePackageManager.1tdu8gu')}</span>
              <div className="package-tabs">
                {packageViews.map((view) => (
                  <button
                    key={view.key}
                    type="button"
                    className={activeView === view.key ? 'active' : ''}
                    onClick={() => loadPackages(view.key)}
                    disabled={loading || managerKind === 'unknown'}
                  >
                    {view.label}
                  </button>
                ))}
              </div>
            </div>
            <div className="package-search-box">
              <span className="package-sidebar-label">{tCurrent('auto.remotePackageManager.1rou4mo')}</span>
              <input
                type="search"
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') void loadPackages('search', managerKind, searchQuery);
                }}
                placeholder={tCurrent('auto.remotePackageManager.wl26vz')}
                aria-label={tCurrent('auto.remotePackageManager.njixii')}
              />
              <button type="button" onClick={() => loadPackages('search', managerKind, searchQuery)} disabled={loading || managerKind === 'unknown'}>
                {tCurrent('auto.remotePackageManager.rarta4')}</button>
            </div>
            {activeView === 'search' ? (
              <div className="package-search-state">
                <span>{tCurrent('auto.remotePackageManager.a5ncle')}</span>
                <strong>{lastSearchQuery || '-'}</strong>
              </div>
            ) : null}
            <div className="package-summary">
              <strong>{packages.length}</strong>
              <span>{activeView === 'upgradable' ? tCurrent('auto.remotePackageManager.1dsteob') : activeView === 'installed' ? tCurrent('auto.remotePackageManager.6prvmi') : tCurrent('auto.remotePackageManager.tiekd8')}</span>
            </div>
          </aside>

          <main className="package-list-panel">
            <div className="package-table-wrap">
              <table className="package-table">
                <thead>
                  <tr>
                    <th>{tCurrent('auto.remotePackageManager.1obbrxz')}</th>
                    <th>{tCurrent('auto.remotePackageManager.va46gx')}</th>
                    <th>{tCurrent('auto.remotePackageManager.1mi4vdj')}</th>
                    <th>{tCurrent('auto.remotePackageManager.1ccx4t4')}</th>
                  </tr>
                </thead>
                <tbody>
                  {packages.map((pkg) => (
                    <tr key={`${pkg.name}-${pkg.version ?? pkg.latestVersion ?? ''}`} className={selectedPackage?.name === pkg.name ? 'selected' : ''} onClick={() => setSelectedName(pkg.name)}>
                      <td><strong>{pkg.name}</strong></td>
                      <td>{getPackageVersion(pkg)}</td>
                      <td title={pkg.description || pkg.source || ''}>{pkg.description || pkg.source || '-'}</td>
                      <td>{pkg.upgradable ? <span className="package-pill warning">{tCurrent('auto.remotePackageManager.13p2xv82')}</span> : pkg.installed ? <span className="package-pill success">{tCurrent('auto.remotePackageManager.hdlzwj3')}</span> : <span className="package-pill">{tCurrent('auto.remotePackageManager.1k3oaj4')}</span>}</td>
                    </tr>
                  ))}
                  {!loading && packages.length === 0 ? (
                    <tr>
                      <td colSpan={4} className="package-empty">{tCurrent('auto.remotePackageManager.13e22uy')}</td>
                    </tr>
                  ) : null}
                </tbody>
              </table>
            </div>
          </main>

          <aside className="package-detail-panel">
            {selectedPackage ? (
              <>
                <div className="package-detail-title">
                  <span>{tCurrent('auto.remotePackageManager.1msktrp')}</span>
                  <strong>{selectedPackage.name}</strong>
                </div>
                <dl>
                  <div><dt>{tCurrent('auto.remotePackageManager.15awyex')}</dt><dd>{selectedPackage.version || '-'}</dd></div>
                  <div><dt>{tCurrent('auto.remotePackageManager.t50ebd')}</dt><dd>{selectedPackage.latestVersion || '-'}</dd></div>
                  <div><dt>{tCurrent('auto.remotePackageManager.1mi4vdj2')}</dt><dd>{selectedPackage.description || selectedPackage.source || '-'}</dd></div>
                </dl>
                <div className="package-detail-actions">
                  <button
                    type="button"
                    className="primary"
                    onClick={() => {
                      if (selectedPrimaryAction?.action) {
                        prepareAction(selectedPrimaryAction.action, selectedPackage);
                      }
                    }}
                    disabled={selectedPrimaryAction?.disabled}
                  >
                    {selectedPrimaryAction?.label ?? tCurrent('auto.remotePackageManager.h3munn3')}
                  </button>
                  <button type="button" className="danger" onClick={() => prepareAction('remove', selectedPackage)} disabled={!selectedPackage.installed}>{tCurrent('auto.remotePackageManager.1lca2z22')}</button>
                  <button type="button" onClick={() => navigator.clipboard.writeText(selectedPackage.name).then(() => setNotice(tCurrent('auto.remotePackageManager.1o8al5y')))}>{tCurrent('auto.remotePackageManager.ggdahc')}</button>
                </div>
                <div className="package-note">
                  {tCurrent('auto.remotePackageManager.igwc6')}</div>
              </>
            ) : (
              <div className="package-empty detail">{tCurrent('auto.remotePackageManager.1c81e3l')}</div>
            )}
          </aside>
        </div>
      )}

      {pendingAction ? createPortal(
        <div className="package-modal-backdrop" role="presentation" onClick={closePendingAction}>
          <div className="package-confirm-dialog" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
            <div className="package-confirm-header">
              <span>{pendingAction.danger ? tCurrent('auto.remotePackageManager.ahn1l4') : tCurrent('auto.remotePackageManager.17ojhw6')}</span>
              <strong>{pendingAction.label}{pendingAction.packageName ? ` ${pendingAction.packageName}` : ''}</strong>
            </div>
            <pre>{pendingAction.preview ?? pendingAction.command}</pre>
            {actionOutputState !== 'idle' || actionOutput ? (
              <div className="package-command-output">
                <div className="package-command-output-header">
                  <strong>{tCurrent('auto.remotePackageManager.1mh8d93')}</strong>
                  <span className={`package-output-state ${actionOutputState}`}>
                    {actionOutputState === 'running' ? tCurrent('auto.remotePackageManager.6svkbt') : actionOutputState === 'success' ? tCurrent('auto.remotePackageManager.19j4h') : actionOutputState === 'error' ? tCurrent('auto.remotePackageManager.omysx0') : tCurrent('auto.remotePackageManager.1oherf4')}
                  </span>
                </div>
                <pre ref={actionOutputRef} aria-live="polite">{actionOutput || tCurrent('auto.remotePackageManager.k2xa6s')}</pre>
              </div>
            ) : null}
            <div className="package-confirm-actions">
              <button type="button" onClick={closePendingAction} disabled={actionRunning}>{actionOutputState === 'idle' ? tCurrent('auto.remotePackageManager.1589w37') : tCurrent('auto.remotePackageManager.g0fanx')}</button>
              <button type="button" onClick={copyPendingCommand}>{tCurrent('auto.remotePackageManager.qxd4qr')}</button>
              <button type="button" onClick={runPendingInTerminal} disabled={!onOpenTerminal || actionRunning}>{tCurrent('auto.remotePackageManager.la3v4c')}</button>
              <button type="button" className={pendingAction.danger ? 'danger' : 'primary'} onClick={executePendingAction} disabled={actionRunning}>
                {actionRunning ? tCurrent('auto.remotePackageManager.6svkbt2') : tCurrent('auto.remotePackageManager.th962h')}
              </button>
            </div>
          </div>
        </div>,
        document.body,
      ) : null}
      {sudoPrompt}
    </section>
  );
}

export default RemotePackageManager;

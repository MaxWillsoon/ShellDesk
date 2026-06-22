import type { MutableRefObject, RefObject } from 'react';

import { isWindowsSystem } from './remoteSystem';
import { getErrorMessage } from './desktopUtils';
import type { TerminalTransferCommand } from './terminalTypes';
import { t } from '../../i18n';

function formatTransferBytes(size: number) {
  if (!Number.isFinite(size) || size < 0) {
    return '-';
  }

  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = size;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function getTransferPercent(payload: Pick<ShellDeskTransferProgress, 'total' | 'transferred' | 'completedItems' | 'totalItems'>) {
  if (payload.total > 0) {
    return Math.max(0, Math.min(100, Math.round((payload.transferred / payload.total) * 100)));
  }

  if ((payload.totalItems ?? 0) > 0) {
    return Math.max(0, Math.min(100, Math.round(((payload.completedItems ?? 0) / (payload.totalItems ?? 1)) * 100)));
  }

  return 0;
}

export function buildSftpProgressText(
  payload: ShellDeskTransferProgress | ShellDeskTransferEndPayload,
  language: ShellDeskAppSettings['language'],
  statusText = '',
) {
  const action = payload.type === 'download'
    ? t('fileExplorer.transfer.download', language)
    : t('fileExplorer.transfer.upload', language);
  const totalText = payload.total > 0 ? ` / ${formatTransferBytes(payload.total)}` : '';
  const itemText = (payload.totalItems ?? 0) > 0
    ? ` · ${t('fileExplorer.transfer.items', language, {
        completed: payload.completedItems ?? 0,
        total: t('fileExplorer.transfer.totalSuffix', language, { total: payload.totalItems ?? 0 }),
      })}`
    : '';
  const statusSuffix = statusText ? ` · ${statusText}` : '';

  return [
    `SFTP ${action}`,
    `${getTransferPercent(payload)}%`,
    `${formatTransferBytes(payload.transferred)}${totalText}`,
    payload.fileName,
  ].filter(Boolean).join(' · ') + itemText + statusSuffix;
}

export function joinTransferRemotePath(basePath: string, remotePath: string, isWindowsHost: boolean) {
  const normalizedRemotePath = isWindowsHost ? remotePath.replace(/\\/g, '/') : remotePath;

  if (isAbsoluteTransferPath(normalizedRemotePath, isWindowsHost)) {
    return normalizedRemotePath;
  }

  const normalizedBasePath = (isWindowsHost ? basePath.replace(/\\/g, '/') : basePath).trim() || '.';

  if (normalizedBasePath === '.') {
    return normalizedRemotePath;
  }

  if (normalizedBasePath === '/') {
    return `/${normalizedRemotePath.replace(/^\/+/u, '')}`;
  }

  if (isWindowsHost && /^\/?[a-z]:\/?$/iu.test(normalizedBasePath)) {
    return `${normalizedBasePath.replace(/\/?$/u, '/')}${normalizedRemotePath.replace(/^\/+/u, '')}`;
  }

  return `${normalizedBasePath.replace(/\/+$/u, '')}/${normalizedRemotePath.replace(/^\/+/u, '')}`;
}

function isAbsoluteTransferPath(remotePath: string, isWindowsHost: boolean) {
  const normalizedPath = remotePath.replace(/\\/g, '/');

  if (normalizedPath.startsWith('~')) {
    return true;
  }

  if (isWindowsHost) {
    return /^\/?[a-z]:\//iu.test(normalizedPath) || normalizedPath.startsWith('/');
  }

  return normalizedPath.startsWith('/');
}

interface SftpTransferRunnerContext {
  api: ShellDeskApi;
  connectionId: string;
  terminalId: string;
  systemType?: import('./types').RemoteSystemType;
  settingsRef: RefObject<ShellDeskAppSettings>;
  activeSftpTransferRef: MutableRefObject<boolean>;
  sftpTransferClientIdRef: MutableRefObject<string>;
  sftpTransferQueueIdRef: MutableRefObject<string>;
  sftpTransferEndedRef: MutableRefObject<boolean>;
  sftpProgressLineLengthRef: MutableRefObject<number>;
  sftpAvailabilityRef: MutableRefObject<{ available: boolean; checkedAt: number } | null>;
  checkSftpAvailability: () => Promise<boolean>;
  resolveTerminalWorkingDirectory: () => Promise<string>;
  writeTerminalInputAsync: (data: string) => Promise<boolean>;
  writeTerminalNotice: (message: string) => void;
  writeSftpProgressLine: (text: string, endLine?: boolean) => void;
  focusTerminal: () => void;
  isDisposed: () => boolean;
}

export function createSftpTransferRunner({
  api,
  connectionId,
  terminalId,
  systemType,
  settingsRef,
  activeSftpTransferRef,
  sftpTransferClientIdRef,
  sftpTransferQueueIdRef,
  sftpTransferEndedRef,
  sftpProgressLineLengthRef,
  sftpAvailabilityRef,
  checkSftpAvailability,
  resolveTerminalWorkingDirectory,
  writeTerminalInputAsync,
  writeTerminalNotice,
  writeSftpProgressLine,
  focusTerminal,
  isDisposed,
}: SftpTransferRunnerContext) {
  return async (transferCommand: TerminalTransferCommand) => {
    let shouldRedrawPrompt = false;
    const transferClientId = `terminal-transfer-${terminalId}-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
    const transferOptions: ShellDeskSudoPasswordOptions = { transferClientId };
    const beginSftpProgress = () => {
      activeSftpTransferRef.current = true;
      sftpTransferClientIdRef.current = transferClientId;
      sftpTransferQueueIdRef.current = '';
      sftpTransferEndedRef.current = false;
      sftpProgressLineLengthRef.current = 0;
    };
    const cancelSftpProgress = () => {
      if (activeSftpTransferRef.current) {
        activeSftpTransferRef.current = false;
        sftpTransferClientIdRef.current = '';
        sftpTransferQueueIdRef.current = '';
        sftpTransferEndedRef.current = false;
        sftpProgressLineLengthRef.current = 0;
      }
    };

    try {
      const isSftpAvailable = await checkSftpAvailability();

      if (isDisposed()) {
        return;
      }

      if (!isSftpAvailable) {
        writeTerminalNotice(t('terminal.transfer.sftpFallback', settingsRef.current.language));
        await writeTerminalInputAsync(transferCommand.inputData);
        return;
      }

      shouldRedrawPrompt = true;
      if (transferCommand.needsLineClear) {
        await writeTerminalInputAsync('\x15');
      }

      const isWindowsHost = isWindowsSystem(systemType);
      const remoteDirectory = await resolveTerminalWorkingDirectory();

      if (isDisposed()) {
        return;
      }

      if (transferCommand.action === 'rz') {
        writeTerminalNotice(t('terminal.transfer.sftpUpload', settingsRef.current.language, { path: remoteDirectory }));
        beginSftpProgress();
        const result = await api.connections.uploadFiles(connectionId, remoteDirectory, transferOptions);

        if (result.canceled) {
          cancelSftpProgress();
        }
        return;
      }

      const remotePaths = transferCommand.remotePaths.map((remotePath) =>
        joinTransferRemotePath(remoteDirectory, remotePath, isWindowsHost));

      writeTerminalNotice(t('terminal.transfer.sftpDownload', settingsRef.current.language, {
        count: String(remotePaths.length),
      }));

      beginSftpProgress();
      const result = remotePaths.length === 1
        ? await api.connections.downloadFile(connectionId, remotePaths[0], transferOptions)
        : await api.connections.downloadPaths(connectionId, remotePaths, transferOptions);

      if (result.canceled) {
        cancelSftpProgress();
      }
    } catch (error) {
      sftpAvailabilityRef.current = null;
      const isAlreadyReportedByProgress = sftpTransferEndedRef.current;
      if (sftpProgressLineLengthRef.current > 0) {
        writeSftpProgressLine('', true);
      }
      cancelSftpProgress();
      if (!isAlreadyReportedByProgress) {
        writeTerminalNotice(t('terminal.transfer.sftpFailed', settingsRef.current.language, {
          error: getErrorMessage(error),
        }));
      }
    } finally {
      if (shouldRedrawPrompt && !isDisposed()) {
        await writeTerminalInputAsync('\r');
      }
      focusTerminal();
    }
  };
}

export function createSftpProgressHandlers({
  connectionId,
  settingsRef,
  activeSftpTransferRef,
  sftpTransferClientIdRef,
  sftpTransferQueueIdRef,
  sftpTransferEndedRef,
  writeSftpProgressLine,
}: {
  connectionId: string;
  settingsRef: RefObject<ShellDeskAppSettings>;
  activeSftpTransferRef: MutableRefObject<boolean>;
  sftpTransferClientIdRef: MutableRefObject<string>;
  sftpTransferQueueIdRef: MutableRefObject<string>;
  sftpTransferEndedRef: MutableRefObject<boolean>;
  writeSftpProgressLine: (text: string, endLine?: boolean) => void;
}) {
  const matchesActiveTransfer = (payload: ShellDeskTransferProgress | ShellDeskTransferEndPayload) => {
    if (!activeSftpTransferRef.current || payload.connectionId !== connectionId) {
      return false;
    }

    if (sftpTransferClientIdRef.current && payload.clientId !== sftpTransferClientIdRef.current) {
      return false;
    }

    return !(sftpTransferQueueIdRef.current && payload.queueId && payload.queueId !== sftpTransferQueueIdRef.current);
  };

  const renderSftpProgress = (payload: ShellDeskTransferProgress) => {
    if (!matchesActiveTransfer(payload)) {
      return;
    }

    if (!sftpTransferQueueIdRef.current && payload.queueId) {
      sftpTransferQueueIdRef.current = payload.queueId;
    }

    writeSftpProgressLine(buildSftpProgressText(payload, settingsRef.current.language));
  };

  const finishSftpProgress = (payload: ShellDeskTransferEndPayload) => {
    if (!matchesActiveTransfer(payload)) {
      return;
    }

    const statusText = payload.success
      ? t('terminal.transfer.sftpDone', settingsRef.current.language)
      : t('terminal.transfer.sftpFailed', settingsRef.current.language, { error: payload.error ?? '' });
    writeSftpProgressLine(buildSftpProgressText(payload, settingsRef.current.language, statusText), true);
    sftpTransferEndedRef.current = true;
    activeSftpTransferRef.current = false;
    sftpTransferClientIdRef.current = '';
    sftpTransferQueueIdRef.current = '';
  };

  return { renderSftpProgress, finishSftpProgress };
}

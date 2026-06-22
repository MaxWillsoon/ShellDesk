import type { MutableRefObject } from 'react';

import { isWindowsSystem } from './remoteSystem';
import { stripTerminalControlSequences } from './terminalCommands';
import { terminalCwdProbeBufferLimit, terminalCwdProbeTimeoutMs } from './terminalCore';
import type { RemoteTerminalLaunchOptions, TerminalCwdProbeState } from './terminalTypes';
import type { RemoteSystemType } from './types';

export function createTerminalCwdProbeController({
  terminalCwdProbeRef,
  launchOptionsRef,
  isTerminalReadyRef,
  systemType,
  writeTerminalInputAsync,
}: {
  terminalCwdProbeRef: MutableRefObject<TerminalCwdProbeState | null>;
  launchOptionsRef: MutableRefObject<RemoteTerminalLaunchOptions | undefined>;
  isTerminalReadyRef: MutableRefObject<boolean>;
  systemType?: RemoteSystemType;
  writeTerminalInputAsync: (data: string) => Promise<boolean>;
}) {
  const settleTerminalCwdProbe = (directory: string) => {
    const probe = terminalCwdProbeRef.current;

    if (!probe) {
      return;
    }

    window.clearTimeout(probe.timer);
    terminalCwdProbeRef.current = null;
    probe.resolve(directory);
  };

  const processTerminalCwdProbeOutput = (data: string) => {
    const probe = terminalCwdProbeRef.current;

    if (!probe) {
      return false;
    }

    const normalizedData = stripTerminalControlSequences(data).replace(/\r/g, '\n');
    const wasCapturing = probe.hasBeginMarker;
    const hasProbeMarker = normalizedData.includes(probe.beginMarker) || normalizedData.includes(probe.endMarker);
    probe.buffer = `${probe.buffer}${normalizedData}`.slice(-terminalCwdProbeBufferLimit);
    const lines = probe.buffer.split(/\n/u).map((line) => line.trim());
    const beginIndex = lines.findIndex((line) => line === probe.beginMarker);

    if (beginIndex < 0) {
      return wasCapturing || hasProbeMarker;
    }

    probe.hasBeginMarker = true;
    const endIndex = lines.findIndex((line, index) => index > beginIndex && line === probe.endMarker);

    if (endIndex < 0) {
      return true;
    }

    const directory = lines
      .slice(beginIndex + 1, endIndex)
      .find((line) => line && line !== probe.beginMarker && line !== probe.endMarker) ?? '';

    settleTerminalCwdProbe(directory);
    return true;
  };

  const createTerminalCwdProbeCommand = (beginMarker: string, endMarker: string) => {
    const shell = launchOptionsRef.current?.shell?.toLowerCase() ?? '';

    if (isWindowsSystem(systemType)) {
      if (/\bcmd(?:\.exe)?\b/u.test(shell)) {
        return `echo ${beginMarker} & cd & echo ${endMarker}`;
      }

      return `Write-Output '${beginMarker}'; (Get-Location).Path; Write-Output '${endMarker}'`;
    }

    return `printf '%s\\n' '${beginMarker}'; pwd -P 2>/dev/null || pwd; printf '%s\\n' '${endMarker}'`;
  };

  const resolveTerminalWorkingDirectory = async () => {
    if (!isTerminalReadyRef.current) {
      return launchOptionsRef.current?.workingDirectory?.trim() || '.';
    }

    const sequence = Math.random().toString(36).slice(2, 10);
    const beginMarker = `__SHELLDESK_CWD_${sequence}_BEGIN__`;
    const endMarker = `__SHELLDESK_CWD_${sequence}_END__`;
    const previousProbe = terminalCwdProbeRef.current;

    if (previousProbe) {
      window.clearTimeout(previousProbe.timer);
      terminalCwdProbeRef.current = null;
      previousProbe.resolve('');
    }

    return new Promise<string>((resolve) => {
      const timer = window.setTimeout(() => {
        settleTerminalCwdProbe('');
      }, terminalCwdProbeTimeoutMs);

      terminalCwdProbeRef.current = {
        beginMarker,
        endMarker,
        buffer: '',
        hasBeginMarker: false,
        timer,
        resolve,
      };

      void writeTerminalInputAsync(`${createTerminalCwdProbeCommand(beginMarker, endMarker)}\r`);
    }).then((directory) => directory || launchOptionsRef.current?.workingDirectory?.trim() || '.');
  };

  return {
    processTerminalCwdProbeOutput,
    resolveTerminalWorkingDirectory,
  };
}

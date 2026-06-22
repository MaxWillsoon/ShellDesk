import type { ForegroundTaskSource } from './terminalTypes';

const outputSummaryLimit = 1200;
const ansiOscPattern = /\x1b\][^\x07]*(?:\x07|\x1b\\)/g;
const ansiCsiPattern = /\x1b\[[0-?]*[ -/]*[@-~]/g;
const controlCharacterPattern = /[\x00-\x08\x0b-\x1f\x7f]/g;
const alternateScreenPattern = /\x1b\[\?(?:47|1047|1049)([hl])/g;
const foregroundCommandPattern = /^(?:(?:sudo|doas)\s+)?(?:top(?!\s+-b(?:\s|$))|htop|btop|atop|watch|vim|vi|nvim|nano|less|more|man)(?:\s|$)/i;
const foregroundSequenceBufferLimit = 32;

export function stripTerminalControlSequences(data: string) {
  return data
    .replace(ansiOscPattern, '')
    .replace(ansiCsiPattern, '')
    .replace(controlCharacterPattern, '')
    .replace(/\r/g, '');
}

export function summarizeTerminalOutput(data: string) {
  const summary = stripTerminalControlSequences(data).trim();

  if (!summary) {
    return null;
  }

  if (summary.length <= outputSummaryLimit) {
    return { summary, truncated: false };
  }

  return {
    summary: summary.slice(-outputSummaryLimit),
    truncated: true,
  };
}

export function readForegroundTaskSignal(previousBuffer: string, data: string) {
  const combinedData = `${previousBuffer}${data}`;
  let hasForegroundTask: boolean | null = null;
  alternateScreenPattern.lastIndex = 0;
  let match: RegExpExecArray | null = alternateScreenPattern.exec(combinedData);

  while (match) {
    hasForegroundTask = match[1] === 'h';
    match = alternateScreenPattern.exec(combinedData);
  }

  alternateScreenPattern.lastIndex = 0;

  return {
    buffer: combinedData.slice(-foregroundSequenceBufferLimit),
    hasForegroundTask,
  };
}

export function isLikelyForegroundCommand(command: string) {
  const trimmedCommand = command.trim();

  if (!trimmedCommand || /[;&|]/.test(trimmedCommand)) {
    return false;
  }

  return foregroundCommandPattern.test(trimmedCommand);
}

export function formatTroubleshootingSnippet(selection: string) {
  return selection
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => (line.trim() ? `$ ${line}` : '$'))
    .join('\n');
}

export function collectSubmittedCommands(currentBuffer: string, data: string) {
  if (data.includes('\x1b')) {
    return { buffer: currentBuffer, commands: [] as string[] };
  }

  let nextBuffer = currentBuffer;
  const commands: string[] = [];

  for (const character of data) {
    if (character === '\r' || character === '\n') {
      const command = nextBuffer.trim();

      if (command) {
        commands.push(command);
      }

      nextBuffer = '';
      continue;
    }

    if (character === '\x7f') {
      nextBuffer = nextBuffer.slice(0, -1);
      continue;
    }

    if (character === '\x03') {
      nextBuffer = '';
      continue;
    }

    if (character >= ' ') {
      nextBuffer += character;
    }
  }

  return { buffer: nextBuffer, commands };
}

export function nextForegroundTaskSource(
  command: string,
  currentSource: ForegroundTaskSource | null,
) {
  return isLikelyForegroundCommand(command) ? 'command' : currentSource;
}

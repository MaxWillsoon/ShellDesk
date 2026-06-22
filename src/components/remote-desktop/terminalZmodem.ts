import type { Terminal as XTerminal } from '@xterm/xterm';
import type { MutableRefObject, RefObject } from 'react';
import * as Zmodem from 'zmodem.js';

import { getErrorMessage } from './desktopUtils';
import { collectSubmittedCommands } from './terminalCommands';
import type { TerminalBufferLineLike, TerminalTransferCommand } from './terminalTypes';
import { t } from '../../i18n';

const zmodemReadChunkSize = 64 * 1024;
const zmodemUploadCommands = new Set(['rz', 'lrz']);
const zmodemDownloadCommands = new Set(['sz', 'lsz']);
const szOptionsWithValue = new Set(['-B', '-L', '-l', '-w', '--bufsize', '--packetlen', '--framelen', '--window-size']);
const szUnsupportedOptions = new Set(['-i', '--command', '-X', '--xmodem', '-Y', '--ymodem']);
const terminalPayloadEncoder = new TextEncoder();

function getCommandBasename(commandPath: string) {
  return commandPath.replace(/\\/g, '/').split('/').pop()?.toLowerCase() ?? '';
}

function pushShellWord(words: string[], word: string, hasWord: boolean) {
  if (hasWord) {
    words.push(word);
  }
}

export function parseSimpleShellWords(command: string) {
  const words: string[] = [];
  let word = '';
  let quote: '"' | "'" | null = null;
  let hasWord = false;

  for (let index = 0; index < command.length; index += 1) {
    const character = command[index];

    if (quote === "'") {
      if (character === "'") {
        quote = null;
      } else {
        word += character;
        hasWord = true;
      }
      continue;
    }

    if (quote === '"') {
      if (character === '"') {
        quote = null;
        continue;
      }

      if (character === '$' || character === '`') {
        return null;
      }

      if (character === '\\') {
        index += 1;
        if (index >= command.length) {
          return null;
        }
        word += command[index];
        hasWord = true;
        continue;
      }

      word += character;
      hasWord = true;
      continue;
    }

    if (/\s/.test(character)) {
      pushShellWord(words, word, hasWord);
      word = '';
      hasWord = false;
      continue;
    }

    if (character === "'" || character === '"') {
      quote = character;
      hasWord = true;
      continue;
    }

    if (/[;&|<>`$(){}]/.test(character)) {
      return null;
    }

    if (character === '\\') {
      index += 1;
      if (index >= command.length) {
        return null;
      }
      word += command[index];
      hasWord = true;
      continue;
    }

    word += character;
    hasWord = true;
  }

  if (quote) {
    return null;
  }

  pushShellWord(words, word, hasWord);
  return words;
}

function optionTakesSeparateValue(option: string) {
  if (szOptionsWithValue.has(option)) {
    return true;
  }

  return /^-[BLlw]$/u.test(option);
}

function optionIncludesValue(option: string) {
  return /^-[BLlw].+/u.test(option) || /^--(?:bufsize|packetlen|framelen|window-size)=/u.test(option);
}

export function readSzRemotePaths(tokens: string[]) {
  const remotePaths: string[] = [];
  let stopParsingOptions = false;

  for (let index = 1; index < tokens.length; index += 1) {
    const token = tokens[index];

    if (!stopParsingOptions && token === '--') {
      stopParsingOptions = true;
      continue;
    }

    if (!stopParsingOptions && szUnsupportedOptions.has(token)) {
      return null;
    }

    if (!stopParsingOptions && optionTakesSeparateValue(token)) {
      index += 1;
      continue;
    }

    if (!stopParsingOptions && (optionIncludesValue(token) || (token.startsWith('-') && token.length > 1))) {
      continue;
    }

    remotePaths.push(token);
  }

  return remotePaths.length ? remotePaths : null;
}

export function readTransferCommand(command: string, inputData: string, needsLineClear: boolean): TerminalTransferCommand | null {
  const tokens = parseSimpleShellWords(command);

  if (!tokens?.length) {
    return null;
  }

  const commandName = getCommandBasename(tokens[0]);

  if (zmodemUploadCommands.has(commandName)) {
    const hasUnexpectedArgument = tokens.slice(1).some((token) => token !== '--' && !token.startsWith('-'));

    return hasUnexpectedArgument
      ? null
      : { action: 'rz', command, inputData, needsLineClear, remotePaths: [] };
  }

  if (zmodemDownloadCommands.has(commandName)) {
    const remotePaths = readSzRemotePaths(tokens);

    return remotePaths
      ? { action: 'sz', command, inputData, needsLineClear, remotePaths }
      : null;
  }

  return null;
}

export function readSubmittedTransferCommand(currentBuffer: string, data: string) {
  const commandState = collectSubmittedCommands(currentBuffer, data);

  if (commandState.commands.length !== 1 || commandState.buffer) {
    return null;
  }

  const command = commandState.commands[0];
  const needsLineClear = currentBuffer.trim().length > 0 && /^[\r\n]+$/u.test(data);

  return readTransferCommand(command, data, needsLineClear);
}

function readVisibleTerminalLine(terminal: XTerminal) {
  const terminalBuffer = (terminal as unknown as {
    buffer?: {
      active?: {
        baseY?: number;
        cursorY?: number;
        getLine?: (lineIndex: number) => TerminalBufferLineLike | undefined;
      };
    };
  }).buffer?.active;

  if (!terminalBuffer?.getLine) {
    return '';
  }

  let lineIndex = Number(terminalBuffer.baseY ?? 0) + Number(terminalBuffer.cursorY ?? 0);
  const parts: string[] = [];

  for (let wrappedLineCount = 0; wrappedLineCount < 8 && lineIndex >= 0; wrappedLineCount += 1) {
    const line = terminalBuffer.getLine(lineIndex);

    if (!line) {
      break;
    }

    parts.unshift(line.translateToString(true));

    if (!line.isWrapped) {
      break;
    }

    lineIndex -= 1;
  }

  return parts.join('').trimEnd();
}

export function readVisibleSubmittedTransferCommand(terminal: XTerminal, data: string) {
  if (!/^[\r\n]+$/u.test(data)) {
    return null;
  }

  const line = readVisibleTerminalLine(terminal);

  if (!line.trim()) {
    return null;
  }

  const candidates = [line.trim()];
  const promptDelimiterPattern = /[#$>%]\s+/gu;
  let match: RegExpExecArray | null = promptDelimiterPattern.exec(line);
  let lastPromptEnd = -1;

  while (match) {
    lastPromptEnd = match.index + match[0].length;
    match = promptDelimiterPattern.exec(line);
  }

  if (lastPromptEnd >= 0) {
    candidates.unshift(line.slice(lastPromptEnd).trim());
  }

  for (const candidate of candidates) {
    const transferCommand = readTransferCommand(candidate, data, true);

    if (transferCommand) {
      return transferCommand;
    }
  }

  return null;
}

function mergeZmodemChunks(chunks: Uint8Array[]) {
  const totalBytes = chunks.reduce((total, chunk) => total + chunk.byteLength, 0);
  const merged = new Uint8Array(totalBytes);
  let offset = 0;

  chunks.forEach((chunk) => {
    merged.set(chunk, offset);
    offset += chunk.byteLength;
  });

  return merged;
}

export function readTerminalPayloadBytes(payload: { data: string; bytes?: ArrayBuffer | ArrayBufferView | number[] }) {
  const { bytes } = payload;

  if (bytes instanceof ArrayBuffer) {
    return new Uint8Array(bytes);
  }

  if (ArrayBuffer.isView(bytes)) {
    return new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  if (Array.isArray(bytes)) {
    return Uint8Array.from(bytes);
  }

  return terminalPayloadEncoder.encode(payload.data);
}

interface TerminalZmodemContext {
  api: ShellDeskApi;
  connectionId: string;
  terminalId: string;
  terminal: XTerminal;
  settingsRef: RefObject<ShellDeskAppSettings>;
  followOutputRef: RefObject<boolean>;
  zmodemSessionRef: MutableRefObject<Zmodem.ZmodemSession | null>;
  isDisposed: () => boolean;
  writeTerminalNotice: (message: string) => void;
}

export function createZmodemSentry({
  api,
  connectionId,
  terminalId,
  terminal,
  settingsRef,
  followOutputRef,
  zmodemSessionRef,
  isDisposed,
  writeTerminalNotice,
}: TerminalZmodemContext) {
  const sendZmodemBytes = (octets: number[] | Uint8Array) => {
    const bytes = octets instanceof Uint8Array ? octets : Uint8Array.from(octets);

    api.connections.writeTerminalBytes(connectionId, terminalId, bytes).catch((error: unknown) => {
      writeTerminalNotice(t('terminal.error.sendFailed', settingsRef.current.language, {
        error: getErrorMessage(error),
      }));
    });
  };

  const closeZmodemSession = async (session: Zmodem.ZmodemSession) => {
    try {
      await session.close();
    } catch {
      session.abort?.();
    }
  };

  const sendZmodemUploadFile = async (
    session: Zmodem.ZmodemSession,
    file: ShellDeskZmodemUploadFile,
    filesRemaining: number,
    bytesRemaining: number,
  ) => {
    const transfer = await session.send_offer({
      name: file.name,
      size: file.size,
      mtime: new Date(file.lastModified),
      files_remaining: filesRemaining,
      bytes_remaining: bytesRemaining,
    });

    if (!transfer) {
      return;
    }

    let offset = transfer.get_offset();

    if (file.size <= offset) {
      await transfer.end(new Uint8Array());
      return;
    }

    while (offset < file.size) {
      const chunkBuffer = await api.connections.readZmodemUploadFile(
        file.id,
        offset,
        Math.min(zmodemReadChunkSize, file.size - offset),
      );
      const chunk = new Uint8Array(chunkBuffer);

      if (!chunk.byteLength) {
        throw new Error('本地文件读取提前结束。');
      }

      offset += chunk.byteLength;

      if (offset >= file.size) {
        await transfer.end(chunk);
      } else {
        transfer.send(chunk);
      }
    }
  };

  const handleZmodemSendSession = async (session: Zmodem.ZmodemSession) => {
    let selectedFileIds: string[] = [];

    try {
      writeTerminalNotice(t('terminal.transfer.zmodemUploadPrompt', settingsRef.current.language));
      const selection = await api.connections.selectZmodemUploadFiles();

      if (isDisposed()) {
        return;
      }

      if (selection.canceled || !selection.files.length) {
        writeTerminalNotice(t('terminal.transfer.zmodemCanceled', settingsRef.current.language));
        session.abort?.();
        return;
      }

      selectedFileIds = selection.files.map((file) => file.id);
      let bytesRemaining = selection.files.reduce((total, file) => total + file.size, 0);

      for (let index = 0; index < selection.files.length; index += 1) {
        const file = selection.files[index];

        await sendZmodemUploadFile(session, file, selection.files.length - index, bytesRemaining);
        bytesRemaining -= file.size;
      }

      await closeZmodemSession(session);
      writeTerminalNotice(t('terminal.transfer.zmodemUploadDone', settingsRef.current.language));
    } catch (error) {
      session.abort?.();
      writeTerminalNotice(t('terminal.transfer.zmodemFailed', settingsRef.current.language, {
        error: getErrorMessage(error),
      }));
    } finally {
      if (selectedFileIds.length) {
        api.connections.releaseZmodemUploadFiles(selectedFileIds).catch(() => undefined);
      }
      terminal.focus();
    }
  };

  const handleZmodemOffer = (offer: Zmodem.Offer) => {
    void (async () => {
      const details = offer.get_details();
      const fileName = details.name || 'download';
      const chunks: Uint8Array[] = [];

      try {
        writeTerminalNotice(t('terminal.transfer.zmodemDownloadPrompt', settingsRef.current.language, { name: fileName }));
        await offer.accept({
          on_input: (chunk) => {
            chunks.push(Uint8Array.from(chunk));
          },
        });

        const merged = mergeZmodemChunks(chunks);
        const result = await api.connections.saveZmodemFile(fileName, merged);

        if (result.canceled) {
          writeTerminalNotice(t('terminal.transfer.zmodemCanceled', settingsRef.current.language));
          return;
        }

        writeTerminalNotice(t('terminal.transfer.zmodemDownloadSaved', settingsRef.current.language, { name: fileName }));
      } catch (error) {
        try {
          offer.skip();
        } catch {
          /* Ignore skip errors after an accepted transfer. */
        }
        writeTerminalNotice(t('terminal.transfer.zmodemFailed', settingsRef.current.language, {
          error: getErrorMessage(error),
        }));
      } finally {
        terminal.focus();
      }
    })();
  };

  const handleZmodemDetection = (detection: Zmodem.Detection) => {
    try {
      const session = detection.confirm();

      zmodemSessionRef.current = session;
      session.on('session_end', () => {
        if (zmodemSessionRef.current === session) {
          zmodemSessionRef.current = null;
        }
      });

      if (session.type === 'send') {
        void handleZmodemSendSession(session);
        return;
      }

      session.on('offer', (offer) => handleZmodemOffer(offer as Zmodem.Offer));
      session.start?.();
    } catch (error) {
      detection.deny();
      writeTerminalNotice(t('terminal.transfer.zmodemFailed', settingsRef.current.language, {
        error: getErrorMessage(error),
      }));
    }
  };

  const terminalOutputDecoder = new TextDecoder();
  const writeTerminalOutputBytes = (octets: number[]) => {
    const text = terminalOutputDecoder.decode(Uint8Array.from(octets), { stream: true });

    if (!text) {
      return;
    }

    terminal.write(text, () => {
      if (followOutputRef.current) {
        terminal.scrollToBottom();
      }
    });
  };

  return new Zmodem.Sentry({
    to_terminal: writeTerminalOutputBytes,
    sender: sendZmodemBytes,
    on_detect: handleZmodemDetection,
    on_retract: () => undefined,
  });
}

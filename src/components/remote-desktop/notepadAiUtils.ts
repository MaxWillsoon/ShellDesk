import { t, type AppLanguage } from '../../i18n';
import type { NotepadAiAction } from './notepadTypes';
import type { RemoteSystemType } from './types';

export const MAX_AI_FILE_CONTEXT_CHARACTERS = 480000;
export const MAX_AI_SELECTION_CHARACTERS = 6000;
export const MAX_AI_ENVIRONMENT_CHARACTERS = 12000;
export const MAX_AI_HISTORY_MESSAGES = 14;

const MAX_AI_FILE_CONTEXT_CHUNK_CHARACTERS = 60000;
const MAX_AI_COMMAND_OUTPUT_CHARACTERS = 12000;

export function createNotepadAiMessageId() {
  return `ai-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function truncateMiddle(content: string, maxLength: number, language: AppLanguage) {
  if (content.length <= maxLength) {
    return content;
  }

  const headLength = Math.floor(maxLength * 0.58);
  const tailLength = Math.max(0, maxLength - headLength - 80);
  return [
    content.slice(0, headLength),
    '',
    t('notepad.truncate.characters', language, { count: content.length - headLength - tailLength }),
    '',
    content.slice(-tailLength),
  ].join('\n');
}

export function limitAiFileContext(content: string, language: AppLanguage) {
  if (content.length <= MAX_AI_FILE_CONTEXT_CHARACTERS) {
    return {
      content,
      truncated: false,
      omittedCharacters: 0,
    };
  }

  const headLength = Math.floor(MAX_AI_FILE_CONTEXT_CHARACTERS * 0.62);
  const tailLength = Math.max(0, MAX_AI_FILE_CONTEXT_CHARACTERS - headLength);
  const omittedCharacters = content.length - headLength - tailLength;

  return {
    content: [
      content.slice(0, headLength),
      '',
      t('notepad.ai.context.limitNotice', language, { count: omittedCharacters }),
      '',
      content.slice(-tailLength),
    ].join('\n'),
    truncated: true,
    omittedCharacters,
  };
}

export function splitAiFileContext(content: string) {
  const chunks: string[] = [];
  let start = 0;

  while (start < content.length) {
    let end = Math.min(start + MAX_AI_FILE_CONTEXT_CHUNK_CHARACTERS, content.length);

    if (end < content.length) {
      const newlineIndex = content.lastIndexOf('\n', end);
      const minimumChunkEnd = start + Math.floor(MAX_AI_FILE_CONTEXT_CHUNK_CHARACTERS * 0.72);

      if (newlineIndex >= minimumChunkEnd) {
        end = newlineIndex + 1;
      }
    }

    chunks.push(content.slice(start, end));
    start = end;
  }

  return chunks.length ? chunks : [''];
}

export function stripAiActionBlocks(content: string) {
  return content
    .replace(/```shelldesk-action\s*[\s\S]*?```/giu, '')
    .replace(/```shelldesk-action[\s\S]*$/iu, '')
    .trim();
}

export function parseAiAction(content: string): NotepadAiAction | undefined {
  const match = /```shelldesk-action\s*([\s\S]*?)```/iu.exec(content);

  if (!match) {
    return undefined;
  }

  try {
    const parsedAction: unknown = JSON.parse(match[1].trim());

    if (!parsedAction || typeof parsedAction !== 'object') {
      return undefined;
    }

    const action = parsedAction as Partial<NotepadAiAction>;

    if (
      (action.type === 'replace_content' ||
        action.type === 'append_content' ||
        action.type === 'insert_at_cursor' ||
        action.type === 'replace_selection') &&
      typeof action.content === 'string'
    ) {
      return {
        type: action.type,
        content: action.content,
        summary: typeof action.summary === 'string' ? action.summary : undefined,
      };
    }

    if (action.type === 'run_command' && typeof action.command === 'string' && action.command.trim()) {
      return {
        type: 'run_command',
        command: action.command.trim(),
        reason: typeof action.reason === 'string' ? action.reason : undefined,
      };
    }
  } catch {
    return undefined;
  }

  return undefined;
}

export function formatCommandResult(
  command: string,
  result: { stdout: string; stderr: string; code: number },
  language: AppLanguage,
) {
  const stdout = result.stdout
    ? truncateMiddle(result.stdout, MAX_AI_COMMAND_OUTPUT_CHARACTERS, language)
    : t('notepad.command.stdout.empty', language);
  const stderr = result.stderr
    ? `\n\n${t('notepad.command.stderr.label', language)}\n${truncateMiddle(result.stderr, 4000, language)}`
    : '';

  return t('notepad.command.result', language, { command, code: result.code, stdout, stderr });
}

export function getEnvironmentProbeCommand(systemType?: RemoteSystemType) {
  if (systemType === 'windows') {
    return [
      'powershell',
      '-NoProfile',
      '-ExecutionPolicy Bypass',
      '-Command',
      '"$ErrorActionPreference=\'SilentlyContinue\';',
      'Write-Output \'# OS\'; Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, OSArchitecture | Format-List | Out-String;',
      'Write-Output \'# PowerShell\'; $PSVersionTable | Out-String;',
      'Write-Output \'# Runtime\'; foreach ($cmd in \'node\',\'python\',\'python3\',\'dotnet\',\'java\',\'go\',\'rustc\',\'php\') { $found = Get-Command $cmd -ErrorAction SilentlyContinue; if ($found) { Write-Output \"## $cmd\"; & $cmd --version 2>&1 | Select-Object -First 3 } };',
      'Write-Output \'# Paths\'; Get-Location | Out-String"',
    ].join(' ');
  }

  return `sh -lc 'printf "# OS\\n"; (cat /etc/os-release 2>/dev/null || sw_vers 2>/dev/null || uname -a); printf "\\n# Kernel\\n"; uname -a 2>/dev/null; printf "\\n# Shell\\n"; printf "%s\\n" "$SHELL"; printf "\\n# Runtime\\n"; for cmd in node npm pnpm yarn python python3 pip pip3 ruby go rustc cargo java javac php composer docker docker-compose nginx apache2 httpd mysql psql sqlite3; do if command -v "$cmd" >/dev/null 2>&1; then printf "## %s\\n" "$cmd"; "$cmd" --version 2>&1 | head -n 3; fi; done; printf "\\n# Working directory\\n"; pwd'`;
}

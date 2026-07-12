import type { RemoteSystemType } from './types';

export function isWindowsSystem(systemType?: RemoteSystemType) {
  return systemType === 'windows';
}

export interface RemoteCommandInput {
  command: string;
  stdin?: string;
}

function createPowerShellScript(script: string) {
  const utf8Prelude = `
try {
$__shelldeskUtf8 = New-Object System.Text.UTF8Encoding $false
[Console]::InputEncoding = $__shelldeskUtf8
[Console]::OutputEncoding = $__shelldeskUtf8
$OutputEncoding = $__shelldeskUtf8
} catch {}
try { chcp.com 65001 > $null } catch {}
$ProgressPreference = 'SilentlyContinue'
$VerbosePreference = 'SilentlyContinue'
$DebugPreference = 'SilentlyContinue'
$InformationPreference = 'SilentlyContinue'
`;

  return `${utf8Prelude}\n${script}`;
}

export function powershellCommand(script: string) {
  const fullScript = createPowerShellScript(script);
  const bytes = new Uint8Array(fullScript.length * 2);

  for (let index = 0; index < fullScript.length; index += 1) {
    const code = fullScript.charCodeAt(index);
    bytes[index * 2] = code & 0xff;
    bytes[index * 2 + 1] = code >> 8;
  }

  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }

  return `powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -OutputFormat Text -EncodedCommand ${btoa(binary)}`;
}

export function powershellStdinCommand(script: string): RemoteCommandInput {
  return {
    command: 'powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -OutputFormat Text -Command -',
    stdin: createPowerShellScript(script),
  };
}

/**
 * Windows PowerShell writes startup/module-load progress to stderr as CLIXML
 * when its output is redirected through SSH. It is informational rather than
 * a command failure and should not be surfaced as an error notification.
 */
export function isPowerShellProgressCliXml(output: string) {
  const value = output.trim();
  return value.startsWith('#< CLIXML')
    && /<Obj\s+S="progress"/i.test(value)
    && !/<(?:Obj|S)\s+S="(?:error|warning)"/i.test(value);
}

export function powershellSingleQuote(value: string) {
  return `'${value.replace(/'/g, "''")}'`;
}

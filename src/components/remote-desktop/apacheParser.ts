import type {
  ApacheConfigFile,
  ApacheDirective,
  ApacheSslConfig,
  ApacheTestResult,
  ApacheVirtualHost,
} from './apacheManagerTypes';

function stableId(value: string) {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = ((hash << 5) - hash + value.charCodeAt(index)) | 0;
  }
  return Math.abs(hash).toString(36);
}

function basename(filePath: string) {
  return filePath.split(/[\\/]/).pop() || filePath;
}

function stripInlineComment(line: string) {
  let quote: string | null = null;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if ((char === '"' || char === "'") && line[index - 1] !== '\\') {
      quote = quote === char ? null : quote ?? char;
      continue;
    }
    if (char === '#' && !quote) return line.slice(0, index);
  }
  return line;
}

function parseDirective(line: string, lineNumber: number): ApacheDirective | null {
  const stripped = stripInlineComment(line).trim();
  if (!stripped || stripped.startsWith('<')) return null;

  const match = stripped.match(/^([A-Za-z][A-Za-z0-9_:-]*)\s*(.*)$/);
  if (!match) return null;

  return {
    name: match[1],
    value: (match[2] ?? '').trim(),
    line: lineNumber,
  };
}

function directiveValue(directives: ApacheDirective[], name: string) {
  return directives.find((directive) => directive.name.toLowerCase() === name.toLowerCase())?.value ?? '';
}

function directiveValues(directives: ApacheDirective[], name: string) {
  return directives
    .filter((directive) => directive.name.toLowerCase() === name.toLowerCase())
    .map((directive) => directive.value)
    .filter(Boolean);
}

function unquote(value: string) {
  const trimmed = value.trim();
  if ((trimmed.startsWith('"') && trimmed.endsWith('"')) || (trimmed.startsWith("'") && trimmed.endsWith("'"))) {
    return trimmed.slice(1, -1);
  }
  return trimmed;
}

function parseListenPorts(value: string) {
  const ports = new Set<string>();
  for (const token of value.split(/\s+/).filter(Boolean)) {
    if (token.startsWith('[')) {
      const ipv6Port = token.match(/\]:(\d+)$/)?.[1];
      if (ipv6Port) ports.add(ipv6Port);
      continue;
    }
    const colonPort = token.match(/:(\d+)$/)?.[1];
    if (colonPort) {
      ports.add(colonPort);
      continue;
    }
    if (/^\d+$/.test(token)) ports.add(token);
  }
  return [...ports];
}

function parseSslConfig(directives: ApacheDirective[]): ApacheSslConfig | null {
  const certificateFile = directiveValue(directives, 'SSLCertificateFile');
  const certificateKeyFile = directiveValue(directives, 'SSLCertificateKeyFile');
  const chainFile = directiveValue(directives, 'SSLCertificateChainFile');

  if (!certificateFile && !certificateKeyFile && !chainFile) return null;

  return {
    certificateFile: unquote(certificateFile),
    certificateKeyFile: unquote(certificateKeyFile),
    chainFile: chainFile ? unquote(chainFile) : null,
  };
}

function parseVirtualHost(
  filePath: string,
  startLine: number,
  endLine: number,
  listenSpec: string,
  directives: ApacheDirective[],
): ApacheVirtualHost {
  return {
    id: `apache-vhost:${stableId(`${filePath}:${startLine}:${listenSpec}`)}`,
    filePath,
    startLine,
    endLine,
    serverName: unquote(directiveValue(directives, 'ServerName')),
    serverAlias: directiveValues(directives, 'ServerAlias').flatMap((value) => value.split(/\s+/).filter(Boolean).map(unquote)),
    documentRoot: unquote(directiveValue(directives, 'DocumentRoot')),
    listenPorts: parseListenPorts(listenSpec),
    sslConfig: parseSslConfig(directives),
    directives,
    isEnabled: !/\.disabled$/i.test(filePath) && !/\/sites-available\//.test(filePath),
    enabledPath: null,
  };
}

export function parseApacheConfig(content: string, filePath: string): ApacheConfigFile {
  const lines = content.split(/\r?\n/);
  const virtualHosts: ApacheVirtualHost[] = [];
  let active: { startLine: number; listenSpec: string; directives: ApacheDirective[] } | null = null;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const lineNumber = index + 1;
    const trimmed = stripInlineComment(line).trim();
    const startMatch = trimmed.match(/^<VirtualHost\s+([^>]*)>/i);

    if (startMatch && !active) {
      active = {
        startLine: lineNumber,
        listenSpec: startMatch[1].trim(),
        directives: [],
      };
      continue;
    }

    if (/^<\/VirtualHost\s*>/i.test(trimmed) && active) {
      virtualHosts.push(parseVirtualHost(filePath, active.startLine, lineNumber, active.listenSpec, active.directives));
      active = null;
      continue;
    }

    if (active) {
      const directive = parseDirective(line, lineNumber);
      if (directive) active.directives.push(directive);
    }
  }

  if (active) {
    virtualHosts.push(parseVirtualHost(filePath, active.startLine, lines.length, active.listenSpec, active.directives));
  }

  return {
    filename: basename(filePath),
    fullPath: filePath,
    rawContent: content,
    virtualHosts,
    lastModified: 0,
    fileSize: content.length,
  };
}

export function parseApacheTestOutput(output: string): ApacheTestResult {
  return {
    success: /syntax ok/i.test(output) || /syntax is ok/i.test(output),
    output,
  };
}

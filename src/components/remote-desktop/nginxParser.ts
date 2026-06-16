import type {
  NginxConfigFile,
  NginxDirective,
  NginxListenDirective,
  NginxLocationBlock,
  NginxServerBlock,
  NginxSslConfig,
  NginxTestError,
  NginxTestResult,
  NginxUpstreamBlock,
  NginxUpstreamServer,
} from './nginxManagerTypes';

type TokenType = 'word' | 'braceOpen' | 'braceClose' | 'semicolon';

interface Token {
  type: TokenType;
  value: string;
  line: number;
}

interface ParsedNode {
  name: string;
  params: string[];
  line: number;
  endLine: number;
  children: ParsedNode[];
}

function stableId(value: string) {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = ((hash << 5) - hash + value.charCodeAt(index)) | 0;
  }
  return Math.abs(hash).toString(36);
}

function tokenize(content: string): Token[] {
  const tokens: Token[] = [];
  let index = 0;
  let line = 1;

  while (index < content.length) {
    const char = content[index];

    if (char === '\n') {
      line += 1;
      index += 1;
      continue;
    }

    if (/\s/.test(char)) {
      index += 1;
      continue;
    }

    if (char === '#') {
      while (index < content.length && content[index] !== '\n') index += 1;
      continue;
    }

    if (char === '{' || char === '}' || char === ';') {
      tokens.push({
        type: char === '{' ? 'braceOpen' : char === '}' ? 'braceClose' : 'semicolon',
        value: char,
        line,
      });
      index += 1;
      continue;
    }

    const tokenLine = line;
    let value = '';

    while (index < content.length) {
      const current = content[index];

      if (current === '\n') {
        break;
      }

      if (current === '\\') {
        value += current;
        index += 1;
        if (index < content.length) {
          value += content[index];
          index += 1;
        }
        continue;
      }

      if (current === '"' || current === "'") {
        const quote = current;
        index += 1;
        while (index < content.length) {
          const quoted = content[index];
          if (quoted === '\n') line += 1;
          if (quoted === '\\' && index + 1 < content.length) {
            value += content[index + 1];
            index += 2;
            continue;
          }
          if (quoted === quote) {
            index += 1;
            break;
          }
          value += quoted;
          index += 1;
        }
        continue;
      }

      if (/\s/.test(current) || current === '{' || current === '}' || current === ';' || current === '#') {
        break;
      }

      value += current;
      index += 1;
    }

    if (value) {
      tokens.push({ type: 'word', value, line: tokenLine });
      continue;
    }

    index += 1;
  }

  return tokens;
}

function parseNodes(tokens: Token[], startIndex = 0): { nodes: ParsedNode[]; nextIndex: number; endLine: number } {
  const nodes: ParsedNode[] = [];
  let index = startIndex;
  let endLine = tokens[startIndex]?.line ?? 1;

  while (index < tokens.length) {
    const token = tokens[index];
    endLine = token.line;

    if (token.type === 'braceClose') {
      return { nodes, nextIndex: index + 1, endLine: token.line };
    }

    if (token.type !== 'word') {
      index += 1;
      continue;
    }

    const name = token.value;
    const params: string[] = [];
    const line = token.line;
    index += 1;

    while (index < tokens.length && tokens[index].type === 'word') {
      params.push(tokens[index].value);
      index += 1;
    }

    if (tokens[index]?.type === 'braceOpen') {
      const childResult = parseNodes(tokens, index + 1);
      nodes.push({ name, params, line, endLine: childResult.endLine, children: childResult.nodes });
      index = childResult.nextIndex;
      continue;
    }

    const directiveEndLine = tokens[index]?.line ?? line;
    if (tokens[index]?.type === 'semicolon') index += 1;
    nodes.push({ name, params, line, endLine: directiveEndLine, children: [] });
  }

  return { nodes, nextIndex: index, endLine };
}

function basename(filePath: string) {
  return filePath.split(/[\\/]/).pop() || filePath;
}

function toDirective(node: ParsedNode): NginxDirective {
  return {
    name: node.name,
    params: node.params,
    line: node.line,
  };
}

function childDirectives(node: ParsedNode) {
  return node.children.filter((child) => child.children.length === 0).map(toDirective);
}

function firstDirectiveValue(node: ParsedNode, name: string) {
  const directive = node.children.find((child) => child.name === name && child.children.length === 0);
  return directive?.params.join(' ') || null;
}

function allDirectiveValues(node: ParsedNode, name: string) {
  return node.children
    .filter((child) => child.name === name && child.children.length === 0)
    .flatMap((child) => child.params);
}

function collectBlocks(nodes: ParsedNode[], name: string): ParsedNode[] {
  return nodes.flatMap((node) => [
    ...(node.name === name ? [node] : []),
    ...collectBlocks(node.children, name),
  ]);
}

function parseListenPort(hostPart: string, params: string[]) {
  if (/^\d+$/.test(hostPart)) return Number(hostPart);

  const ipv6Port = hostPart.match(/^\[[^\]]+\]:(\d+)$/)?.[1];
  if (ipv6Port) return Number(ipv6Port);

  const colonPort = hostPart.match(/:(\d+)$/)?.[1];
  if (colonPort) return Number(colonPort);

  return params.includes('ssl') ? 443 : 80;
}

function parseListenAddress(hostPart: string) {
  if (/^\d+$/.test(hostPart)) return '*';
  const ipv6 = hostPart.match(/^(\[[^\]]+\])(?::\d+)?$/)?.[1];
  if (ipv6) return ipv6;
  if (hostPart.includes(':')) return hostPart.replace(/:\d+$/, '') || '*';
  return hostPart || '*';
}

function parseListenDirective(node: ParsedNode): NginxListenDirective {
  const raw = node.params.join(' ');
  const hostPart = node.params[0] ?? '';
  const options = node.params.slice(1);

  return {
    address: parseListenAddress(hostPart),
    port: parseListenPort(hostPart, options),
    ssl: node.params.includes('ssl'),
    http2: node.params.includes('http2'),
    defaultServer: node.params.includes('default_server'),
    raw,
  };
}

function parseSslConfig(node: ParsedNode): NginxSslConfig | null {
  const certificate = firstDirectiveValue(node, 'ssl_certificate');
  const certificateKey = firstDirectiveValue(node, 'ssl_certificate_key');

  if (!certificate && !certificateKey) return null;

  return {
    certificate: certificate ?? '',
    certificateKey: certificateKey ?? '',
    protocols: allDirectiveValues(node, 'ssl_protocols'),
    ciphers: firstDirectiveValue(node, 'ssl_ciphers'),
    hsts: node.children.some((child) => (
      child.name === 'add_header'
      && child.params[0]?.toLowerCase() === 'strict-transport-security'
    )),
  };
}

function parseLocationModifier(params: string[]): NginxLocationBlock['modifier'] {
  const first = params[0];
  if (first === '=' || first === '~' || first === '~*' || first === '^~') return first;
  if (first?.startsWith('@')) return '@';
  return '';
}

function parseLocationPath(params: string[]) {
  const modifier = parseLocationModifier(params);
  if (modifier === '@') return params[0] ?? '';
  if (modifier) return params.slice(1).join(' ');
  return params.join(' ');
}

function parseLocationBlock(node: ParsedNode, filePath: string): NginxLocationBlock {
  return {
    id: `nginx-location:${stableId(`${filePath}:${node.line}:${node.params.join(' ')}`)}`,
    modifier: parseLocationModifier(node.params),
    path: parseLocationPath(node.params),
    proxyPass: firstDirectiveValue(node, 'proxy_pass'),
    fastcgiPass: firstDirectiveValue(node, 'fastcgi_pass'),
    root: firstDirectiveValue(node, 'root'),
    alias: firstDirectiveValue(node, 'alias'),
    tryFiles: node.children.find((child) => child.name === 'try_files' && child.children.length === 0)?.params ?? null,
    rawDirectives: childDirectives(node),
    nestedLocations: node.children
      .filter((child) => child.name === 'location')
      .map((child) => parseLocationBlock(child, filePath)),
    startLine: node.line,
    endLine: node.endLine,
  };
}

function parseServerBlock(node: ParsedNode, filePath: string): NginxServerBlock {
  return {
    id: `nginx-server:${stableId(`${filePath}:${node.line}`)}`,
    configPath: filePath,
    startLine: node.line,
    endLine: node.endLine,
    serverNames: allDirectiveValues(node, 'server_name'),
    listenDirectives: node.children
      .filter((child) => child.name === 'listen' && child.children.length === 0)
      .map(parseListenDirective),
    locations: node.children
      .filter((child) => child.name === 'location')
      .map((child) => parseLocationBlock(child, filePath)),
    sslConfig: parseSslConfig(node),
    root: firstDirectiveValue(node, 'root'),
    index: firstDirectiveValue(node, 'index'),
    accessLog: firstDirectiveValue(node, 'access_log'),
    errorLog: firstDirectiveValue(node, 'error_log'),
    rawDirectives: childDirectives(node),
  };
}

function parseUpstreamServer(node: ParsedNode): NginxUpstreamServer {
  const params = node.params.slice(1);
  const optionValue = (name: string) => params.find((param) => param.startsWith(`${name}=`))?.slice(name.length + 1) ?? null;
  const numericOption = (name: string) => {
    const value = optionValue(name);
    if (value === null) return null;
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  };

  return {
    address: node.params[0] ?? '',
    weight: numericOption('weight'),
    maxFails: numericOption('max_fails'),
    failTimeout: optionValue('fail_timeout'),
    backup: params.includes('backup'),
    down: params.includes('down'),
    raw: node.params.join(' '),
  };
}

function parseUpstreamBlock(node: ParsedNode, filePath: string): NginxUpstreamBlock {
  const method = node.children.find((child) => (
    child.children.length === 0 && ['least_conn', 'ip_hash', 'hash', 'random'].includes(child.name)
  ))?.name ?? 'round_robin';

  return {
    id: `nginx-upstream:${stableId(`${filePath}:${node.line}:${node.params.join(' ')}`)}`,
    name: node.params[0] ?? '',
    method,
    servers: node.children
      .filter((child) => child.name === 'server' && child.children.length === 0)
      .map(parseUpstreamServer),
    keepalive: (() => {
      const value = firstDirectiveValue(node, 'keepalive');
      if (value === null) return null;
      const parsed = Number(value);
      return Number.isFinite(parsed) ? parsed : null;
    })(),
    rawDirectives: childDirectives(node),
    configPath: filePath,
    startLine: node.line,
  };
}

export function parseNginxConfig(content: string, filePath: string): NginxConfigFile {
  const { nodes } = parseNodes(tokenize(content));
  const serverNodes = collectBlocks(nodes, 'server');
  const upstreamNodes = collectBlocks(nodes, 'upstream');

  return {
    filename: basename(filePath),
    fullPath: filePath,
    isEnabled: !/\.disabled$/i.test(filePath) && !/\/sites-available\//.test(filePath),
    enabledPath: null,
    serverBlocks: serverNodes
      .map((node) => parseServerBlock(node, filePath)),
    upstreamBlocks: upstreamNodes
      .map((node) => parseUpstreamBlock(node, filePath)),
    rawContent: content,
    lastModified: 0,
    fileSize: content.length,
  };
}

export function parseNginxTestOutput(output: string): NginxTestResult {
  const errors: NginxTestError[] = [];
  const regex = /\[emerg\].*? in ([^:\s]+):(\d+)|nginx:\s*\[emerg\]\s*(.*?) in ([^:\s]+):(\d+)|"([^"]+)" failed .*? in ([^:\s]+):(\d+)/gi;
  let match: RegExpExecArray | null;

  while ((match = regex.exec(output)) !== null) {
    const file = match[1] ?? match[4] ?? match[7] ?? '';
    const line = Number(match[2] ?? match[5] ?? match[8] ?? 0);
    const message = (match[3] ?? match[6] ?? match[0]).trim();
    errors.push({ file, line, message });
  }

  return {
    success: /syntax is ok/i.test(output) && /test is successful/i.test(output),
    output,
    errors,
  };
}

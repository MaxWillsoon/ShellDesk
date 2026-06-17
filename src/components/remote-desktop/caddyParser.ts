import type { CaddyConfigFile, CaddyDirective, CaddySiteBlock, CaddyTestError, CaddyTestResult } from './caddyManagerTypes';

type TokenType = 'word' | 'braceOpen' | 'braceClose' | 'newline';

interface Token {
  type: TokenType;
  value: string;
  line: number;
}

interface ParsedLine {
  words: string[];
  line: number;
  children: ParsedLine[];
  endLine: number;
}

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

function tokenize(content: string): Token[] {
  const tokens: Token[] = [];
  let index = 0;
  let line = 1;

  while (index < content.length) {
    const char = content[index];

    if (char === '\n') {
      tokens.push({ type: 'newline', value: '\n', line });
      line += 1;
      index += 1;
      continue;
    }

    if (/\s/.test(char)) {
      index += 1;
      continue;
    }

    if (char === '#' || (char === '/' && content[index + 1] === '/')) {
      while (index < content.length && content[index] !== '\n') index += 1;
      continue;
    }

    if (char === '<' && content[index + 1] === '<') {
      const startLine = line;
      let header = '';
      while (index < content.length && content[index] !== '\n') {
        header += content[index];
        index += 1;
      }
      const delimiter = header.replace(/^<<-?/, '').trim().replace(/^['"]|['"]$/g, '');
      tokens.push({ type: 'word', value: header.trim(), line: startLine });
      if (content[index] === '\n') {
        tokens.push({ type: 'newline', value: '\n', line });
        line += 1;
        index += 1;
      }
      while (index < content.length) {
        let currentLine = '';
        while (index < content.length && content[index] !== '\n') {
          currentLine += content[index];
          index += 1;
        }
        if (currentLine.trim() === delimiter) {
          if (content[index] === '\n') {
            line += 1;
            index += 1;
          }
          break;
        }
        if (content[index] === '\n') {
          line += 1;
          index += 1;
        }
      }
      continue;
    }

    if (char === '{' && (/\s/.test(content[index + 1] ?? '') || content[index + 1] === undefined)) {
      tokens.push({ type: 'braceOpen', value: char, line });
      index += 1;
      continue;
    }

    if (char === '}') {
      tokens.push({ type: 'braceClose', value: char, line });
      index += 1;
      continue;
    }

    const tokenLine = line;
    let value = '';
    while (index < content.length) {
      const current = content[index];
      if (current === '\n' || /\s/.test(current)) break;
      if (current === '{' && !/\s/.test(content[index + 1] ?? '')) {
        value += current;
        index += 1;
        continue;
      }
      if ((current === '{' || current === '}') && !value) break;
      if (current === '}' && value) {
        value += current;
        index += 1;
        continue;
      }
      if (current === '#' || (current === '/' && content[index + 1] === '/')) break;
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
      if (current === '\\' && index + 1 < content.length) {
        value += content[index + 1];
        index += 2;
        continue;
      }
      value += current;
      index += 1;
    }
    if (value) tokens.push({ type: 'word', value, line: tokenLine });
    else index += 1;
  }

  return tokens;
}

function parseLines(tokens: Token[], startIndex = 0): { lines: ParsedLine[]; nextIndex: number; endLine: number } {
  const lines: ParsedLine[] = [];
  let index = startIndex;
  let endLine = tokens[startIndex]?.line ?? 1;

  while (index < tokens.length) {
    while (tokens[index]?.type === 'newline') index += 1;
    const token = tokens[index];
    if (!token) break;
    endLine = token.line;
    if (token.type === 'braceClose') return { lines, nextIndex: index + 1, endLine: token.line };
    if (token.type === 'braceOpen') {
      const childResult = parseLines(tokens, index + 1);
      lines.push({ words: [], line: token.line, children: childResult.lines, endLine: childResult.endLine });
      index = childResult.nextIndex;
      continue;
    }
    if (token.type !== 'word') {
      index += 1;
      continue;
    }

    const words: string[] = [];
    const line = token.line;
    while (tokens[index]?.type === 'word') {
      words.push(tokens[index].value);
      index += 1;
    }

    if (tokens[index]?.type === 'braceOpen') {
      const childResult = parseLines(tokens, index + 1);
      lines.push({ words, line, children: childResult.lines, endLine: childResult.endLine });
      index = childResult.nextIndex;
      continue;
    }

    lines.push({ words, line, children: [], endLine: tokens[index]?.line ?? line });
    while (tokens[index]?.type === 'newline') index += 1;
  }

  return { lines, nextIndex: index, endLine };
}

function toDirective(line: ParsedLine): CaddyDirective {
  return {
    name: line.words[0] ?? '',
    args: line.words.slice(1),
    block: line.children.length ? line.children.map(toDirective) : null,
    line: line.line,
  };
}

function isGlobalOptionsBlock(line: ParsedLine, index: number) {
  return index === 0 && line.words.length === 0 && line.children.length > 0;
}

function isSiteBlock(line: ParsedLine, index: number) {
  if (!line.children.length || isGlobalOptionsBlock(line, index)) return false;
  const first = line.words[0] ?? '';
  if (!first || first.startsWith('@') || first.startsWith('(')) return false;
  // Heuristic: Caddyfile site detection is context-sensitive; this blacklist covers most real-world configs.
  if (['handle', 'handle_path', 'route', 'reverse_proxy', 'header', 'log', 'tls', 'encode', 'file_server', 'root', 'php_fastcgi', 'redir', 'respond', 'snippets', 'import', 'order', 'experimental_http3'].includes(first)) return false;
  return true;
}

function siteHasTls(line: ParsedLine, listen: string[]) {
  if (listen.some((item) => item.includes(':443') || item === 'https://')) return true;
  return line.children.some((child) => child.words[0] === 'tls');
}

export function parseCaddyConfig(content: string, filePath: string): CaddyConfigFile {
  const parsed = parseLines(tokenize(content));
  const siteBlocks: CaddySiteBlock[] = [];
  const globalDirectives: CaddyDirective[] = [];

  parsed.lines.forEach((line, index) => {
    if (isGlobalOptionsBlock(line, index)) {
      globalDirectives.push(...line.children.map(toDirective));
      return;
    }
    if (!isSiteBlock(line, index)) {
      globalDirectives.push(toDirective(line));
      return;
    }

    const matcher = line.words.join(' ');
    const listen = line.words.filter((word) => word.startsWith(':') || /^[a-z]+:\/\//i.test(word));
    siteBlocks.push({
      id: stableId(`${filePath}:${line.line}:${matcher}`),
      matcher,
      listen,
      tls: siteHasTls(line, listen),
      directives: line.children.map(toDirective),
      filePath,
      startLine: line.line,
      endLine: line.endLine,
    });
  });

  return {
    filename: basename(filePath),
    fullPath: filePath,
    rawContent: content,
    siteBlocks,
    globalDirectives,
    lastModified: 0,
    fileSize: content.length,
  };
}

export function parseCaddyTestOutput(output: string): CaddyTestResult {
  const errors: CaddyTestError[] = [];
  for (const line of output.split(/\r?\n/)) {
    const match = line.match(/(?:Error|error).*?([^:\s]+Caddyfile|\/[^:\s]+):(\d+)(?::\d+)?:?\s*(.*)$/)
      ?? line.match(/([^:\s]+):(\d+):\s*(.*)$/);
    if (match) {
      errors.push({ file: match[1], line: Number(match[2]), message: match[3] || line });
    }
  }

  return {
    success: !/error|failed|invalid/i.test(output),
    output: output.trim(),
    errors,
  };
}

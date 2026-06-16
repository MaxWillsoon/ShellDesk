import type { NginxDirective, NginxLocationBlock, NginxServerBlock } from './nginxManagerTypes';

export type NginxVisualLocationKind = 'static' | 'proxy' | 'fastcgi' | 'custom';
export type NginxVisualSiteKind = 'static' | 'proxy' | 'php' | 'custom';

export interface NginxVisualLocationForm {
  id: string;
  modifier: NginxLocationBlock['modifier'];
  path: string;
  kind: NginxVisualLocationKind;
  proxyPass: string;
  fastcgiPass: string;
  root: string;
  alias: string;
  tryFiles: string;
  extraDirectives: NginxDirective[];
  nestedLocations: NginxVisualLocationForm[];
}

export interface NginxVisualServerForm {
  siteKind: NginxVisualSiteKind;
  serverNames: string;
  listen: string;
  root: string;
  index: string;
  accessLog: string;
  errorLog: string;
  sslEnabled: boolean;
  sslCertificate: string;
  sslCertificateKey: string;
  extraDirectives: NginxDirective[];
  locations: NginxVisualLocationForm[];
}

const visualServerDirectives = new Set([
  'access_log',
  'error_log',
  'index',
  'listen',
  'root',
  'server_name',
  'ssl_certificate',
  'ssl_certificate_key',
]);

const visualLocationDirectives = new Set([
  'alias',
  'fastcgi_pass',
  'proxy_pass',
  'root',
  'try_files',
]);

function getListenText(block: NginxServerBlock) {
  return block.listenDirectives
    .map((listen) => listen.raw || `${listen.address === '*' ? '' : `${listen.address}:`}${listen.port}${listen.ssl ? ' ssl' : ''}${listen.http2 ? ' http2' : ''}${listen.defaultServer ? ' default_server' : ''}`.trim())
    .filter(Boolean)
    .join('\n');
}

function getLocationKind(location: NginxLocationBlock): NginxVisualLocationKind {
  if (location.proxyPass) return 'proxy';
  if (location.fastcgiPass) return 'fastcgi';
  if (location.root || location.alias || location.tryFiles?.length) return 'static';
  return 'custom';
}

function getSiteKind(block: NginxServerBlock): NginxVisualSiteKind {
  if (block.locations.some((location) => location.fastcgiPass)) return 'php';
  const rootLocation = block.locations.find((location) => location.path === '/' && location.modifier === '');
  if (rootLocation?.proxyPass) return 'proxy';
  if (block.locations.some((location) => location.proxyPass || location.fastcgiPass)) return 'custom';
  return 'static';
}

function preserveDirectives(directives: NginxDirective[], visualDirectives: Set<string>) {
  return directives.filter((directive) => !visualDirectives.has(directive.name));
}

function createLocationForm(location: NginxLocationBlock): NginxVisualLocationForm {
  return {
    id: location.id,
    modifier: location.modifier,
    path: location.path,
    kind: getLocationKind(location),
    proxyPass: location.proxyPass ?? '',
    fastcgiPass: location.fastcgiPass ?? '',
    root: location.root ?? '',
    alias: location.alias ?? '',
    tryFiles: location.tryFiles?.join(' ') ?? '',
    extraDirectives: preserveDirectives(location.rawDirectives, visualLocationDirectives),
    nestedLocations: location.nestedLocations.map(createLocationForm),
  };
}

export function createNginxVisualServerForm(block: NginxServerBlock): NginxVisualServerForm {
  return {
    siteKind: getSiteKind(block),
    serverNames: block.serverNames.join(' '),
    listen: getListenText(block),
    root: block.root ?? '',
    index: block.index ?? '',
    accessLog: block.accessLog ?? '',
    errorLog: block.errorLog ?? '',
    sslEnabled: Boolean(block.sslConfig) || block.listenDirectives.some((listen) => listen.ssl || listen.port === 443),
    sslCertificate: block.sslConfig?.certificate ?? '',
    sslCertificateKey: block.sslConfig?.certificateKey ?? '',
    extraDirectives: preserveDirectives(block.rawDirectives, visualServerDirectives),
    locations: block.locations.map(createLocationForm),
  };
}

export function createEmptyNginxVisualLocationForm(kind: NginxVisualLocationKind = 'proxy', path = '/'): NginxVisualLocationForm {
  return {
    id: `visual-location:${Date.now().toString(36)}:${Math.random().toString(36).slice(2)}`,
    modifier: '',
    path,
    kind,
    proxyPass: kind === 'proxy' ? 'http://127.0.0.1:3000' : '',
    fastcgiPass: kind === 'fastcgi' ? 'unix:/run/php/php-fpm.sock' : '',
    root: kind === 'static' ? '/var/www/html' : '',
    alias: '',
    tryFiles: kind === 'static' ? '$uri $uri/ =404' : '',
    extraDirectives: [],
    nestedLocations: [],
  };
}

function directiveLine(indent: string, name: string, value: string) {
  const trimmed = value.trim();
  return trimmed ? `${indent}${name} ${trimmed};` : '';
}

function preservedDirectiveLine(indent: string, directive: NginxDirective) {
  return directiveLine(indent, directive.name, directive.params.map((param) => (
    /[\s{};#'"]/u.test(param) ? `"${param.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"` : param
  )).join(' '));
}

function getListenLines(value: string) {
  return value
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean);
}

function pushIfPresent(lines: string[], value: string) {
  if (value) lines.push(value);
}

function renderLocation(form: NginxVisualLocationForm, depth: number) {
  const indent = '  '.repeat(depth);
  const childIndent = '  '.repeat(depth + 1);
  const modifier = form.modifier && form.modifier !== '@' ? `${form.modifier} ` : '';
  const path = form.modifier === '@'
    ? (form.path.trim().startsWith('@') ? form.path.trim() : `@${form.path.trim().replace(/^@/u, '') || 'named'}`)
    : form.path.trim() || '/';
  const lines = [`${indent}location ${modifier}${path} {`];

  if (form.kind === 'proxy') {
    pushIfPresent(lines, directiveLine(childIndent, 'proxy_pass', form.proxyPass));
  }
  if (form.kind === 'fastcgi') {
    pushIfPresent(lines, directiveLine(childIndent, 'fastcgi_pass', form.fastcgiPass));
  }
  if (form.kind === 'static') {
    pushIfPresent(lines, directiveLine(childIndent, 'root', form.root));
    pushIfPresent(lines, directiveLine(childIndent, 'alias', form.alias));
    pushIfPresent(lines, directiveLine(childIndent, 'try_files', form.tryFiles));
  }

  form.extraDirectives.forEach((directive) => pushIfPresent(lines, preservedDirectiveLine(childIndent, directive)));
  form.nestedLocations.forEach((location) => lines.push(renderLocation(location, depth + 1)));
  lines.push(`${indent}}`);
  return lines.join('\n');
}

export function renderNginxVisualServerBlock(form: NginxVisualServerForm) {
  const lines = ['server {'];
  const bodyIndent = '  ';
  const listenLines = getListenLines(form.listen);

  (listenLines.length ? listenLines : ['80']).forEach((listen) => {
    lines.push(directiveLine(bodyIndent, 'listen', listen));
  });
  pushIfPresent(lines, directiveLine(bodyIndent, 'server_name', form.serverNames || '_'));
  pushIfPresent(lines, directiveLine(bodyIndent, 'root', form.root));
  pushIfPresent(lines, directiveLine(bodyIndent, 'index', form.index));
  pushIfPresent(lines, directiveLine(bodyIndent, 'access_log', form.accessLog));
  pushIfPresent(lines, directiveLine(bodyIndent, 'error_log', form.errorLog));

  if (form.sslEnabled) {
    pushIfPresent(lines, directiveLine(bodyIndent, 'ssl_certificate', form.sslCertificate));
    pushIfPresent(lines, directiveLine(bodyIndent, 'ssl_certificate_key', form.sslCertificateKey));
  }

  form.extraDirectives.forEach((directive) => pushIfPresent(lines, preservedDirectiveLine(bodyIndent, directive)));
  form.locations.forEach((location) => {
    if (lines[lines.length - 1] !== '') lines.push('');
    lines.push(renderLocation(location, 1));
  });
  lines.push('}');
  return `${lines.join('\n')}\n`;
}

export function replaceNginxServerBlock(content: string, block: NginxServerBlock, form: NginxVisualServerForm) {
  const newline = content.includes('\r\n') ? '\r\n' : '\n';
  const lines = content.split(/\r?\n/u);
  const replacement = renderNginxVisualServerBlock(form).trimEnd().split('\n');
  lines.splice(block.startLine - 1, Math.max(1, block.endLine - block.startLine + 1), ...replacement);
  return lines.join(newline);
}

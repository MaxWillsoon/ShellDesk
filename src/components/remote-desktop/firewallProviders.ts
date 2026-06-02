import { powershellCommand, powershellSingleQuote } from './remoteSystem';

export type FirewallBackend = 'ufw' | 'firewalld' | 'windows' | 'unknown';
export type FirewallAction = 'allow' | 'deny' | 'reject' | 'limit' | 'unknown';
export type FirewallProtocol = 'tcp' | 'udp' | 'any';

export interface FirewallRule {
  id: string;
  action: FirewallAction;
  protocol?: FirewallProtocol;
  port?: string;
  source?: string;
  target?: string;
  direction?: string;
  raw: string;
}

export interface FirewallSnapshot {
  backend: FirewallBackend;
  status: string;
  defaultPolicy: string;
  zone?: string;
  rules: FirewallRule[];
  rawOutput: string;
}

export interface FirewallRuleDraft {
  action: 'allow' | 'deny' | 'reject';
  protocol: FirewallProtocol;
  port: string;
  source: string;
}

const backendMarker = '__SHELLDESK_FIREWALL_BACKEND__';
const stateMarker = '__SHELLDESK_FIREWALL_STATE__';
const zoneMarker = '__SHELLDESK_FIREWALL_ZONE__';
const listMarker = '__SHELLDESK_FIREWALL_LIST__';
const ufwAddedMarker = '__SHELLDESK_UFW_ADDED__';

function shellSingleQuote(value: string) {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function readString(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];

    if (typeof value === 'string') {
      return value.trim();
    }

    if (typeof value === 'number' || typeof value === 'boolean') {
      return String(value);
    }
  }

  return '';
}

function toRecords(value: unknown): Record<string, unknown>[] {
  const rows = Array.isArray(value) ? value : [value];
  return rows.filter((row): row is Record<string, unknown> => Boolean(row) && typeof row === 'object' && !Array.isArray(row));
}

function normalizeAction(value: string): FirewallAction {
  const normalized = value.trim().toLowerCase();

  if (/\b(?:accept|allow|allowed)\b/.test(normalized)) return 'allow';
  if (normalized === '允许') return 'allow';
  if (normalized === 'deny' || normalized === 'block' || normalized === 'blocked' || normalized.includes('拒绝') || normalized.includes('阻止')) return 'deny';
  if (normalized === 'reject' || normalized.includes('拒收')) return 'reject';
  if (normalized === 'limit' || normalized === 'limited' || normalized.includes('限制') || normalized.includes('限速')) return 'limit';
  return 'unknown';
}

function normalizeProtocol(value?: string): FirewallProtocol | undefined {
  const normalized = String(value ?? '').trim().toLowerCase();

  if (normalized === 'tcp' || normalized === 'udp') return normalized;
  if (normalized === 'any' || normalized === '*' || normalized === '') return 'any';
  return undefined;
}

function parsePortAndProtocol(value: string) {
  const trimmed = value.trim().replace(/\s+\(v6\)$/i, '');
  const match = trimmed.match(/^(.+?)\/(tcp|udp|any)$/i);

  if (!match) {
    return { port: trimmed || undefined, protocol: undefined };
  }

  return {
    port: match[1],
    protocol: normalizeProtocol(match[2]),
  };
}

function parseUfwStatusRuleLine(line: string, index: number): FirewallRule | null {
  if (
    !line
    || line.startsWith(backendMarker)
    || line.startsWith(ufwAddedMarker)
    || /^(?:Status|状态):|^(?:Default|默认):|^(?:Logging|日志):|^(?:New profiles|新配置文件):|^(?:To\s+Action\s+From|至\s+动作\s+来自)|^-+\s+-+\s+-+/i.test(line)
    || /^ERROR:|^sudo:|^WARN/i.test(line)
  ) {
    return null;
  }

  const numberedPrefix = line.match(/^\[\s*(\d+)\]\s*(.+)$/);
  const ruleNumber = numberedPrefix?.[1];
  const body = (numberedPrefix?.[2] ?? line).trim();
  const columns = body.split(/\s{2,}/).map((part) => part.trim()).filter(Boolean);
  let targetText = '';
  let actionText = '';
  let directionText = '';
  let sourceText = '';

  if (columns.length >= 3) {
    targetText = columns[0];
    const actionParts = columns[1].split(/\s+/).filter(Boolean);
    actionText = actionParts[0] ?? '';
    directionText = actionParts.slice(1).join(' ');
    sourceText = columns.slice(2).join(' ');
  } else {
    const fallbackMatch = body.match(/^(.+?)\s+(ALLOW|DENY|REJECT|LIMIT)(?:\s+(IN|OUT|FWD))?\s+(.+)$/i);

    if (!fallbackMatch) {
      return null;
    }

    targetText = fallbackMatch[1] ?? '';
    actionText = fallbackMatch[2] ?? '';
    directionText = fallbackMatch[3] ?? '';
    sourceText = fallbackMatch[4] ?? '';
  }

  const action = normalizeAction(actionText);

  if (action === 'unknown') {
    return null;
  }

  const parsed = parsePortAndProtocol(targetText);

  return {
    id: ruleNumber ? `ufw:${ruleNumber}` : `ufw:raw:${index}`,
    action,
    protocol: parsed.protocol,
    port: parsed.port,
    source: sourceText.trim() || undefined,
    target: targetText.trim() || undefined,
    direction: directionText.trim() || undefined,
    raw: line,
  };
}

function parseUfwAddedRuleLine(line: string, index: number): FirewallRule | null {
  const match = line.trim().match(/^ufw\s+(?:(route)\s+)?(allow|deny|reject|limit)\b\s*(.*)$/i);

  if (!match) {
    return null;
  }

  const routeText = match[1];
  const actionText = match[2] ?? '';
  const rest = (match[3] ?? '').replace(/\s+comment\s+(['"]).*?\1\s*$/i, '').trim();
  const sourceMatch = rest.match(/\bfrom\s+(.+?)(?=\s+\bto\b|\s+\bport\b|\s+\bproto\b|$)/i);
  const targetMatch = rest.match(/\bto\s+(.+?)(?=\s+\bport\b|\s+\bproto\b|$)/i);
  const portMatch = rest.match(/\bport\s+([^\s]+)(?:\s*\/\s*(tcp|udp|any))?/i);
  const protoMatch = rest.match(/\bproto\s+(tcp|udp|any)\b/i);
  const directionMatch = rest.match(/\b(in|out)\b/i);
  const directTarget = !/\b(from|to|port|proto|in|out|on)\b/i.test(rest)
    ? rest.trim()
    : rest
      .replace(/\bfrom\s+.+?(?=\s+\bto\b|\s+\bport\b|\s+\bproto\b|$)/i, ' ')
      .replace(/\bto\s+.+?(?=\s+\bport\b|\s+\bproto\b|$)/i, ' ')
      .replace(/\bport\s+[^\s]+(?:\s*\/\s*(?:tcp|udp|any))?/i, ' ')
      .replace(/\bproto\s+(?:tcp|udp|any)\b/i, ' ')
      .replace(/\b(?:in|out)\b(?:\s+on\s+\S+)?/ig, ' ')
      .replace(/\bon\s+\S+/ig, ' ')
      .trim()
      .split(/\s+/)[0] ?? '';
  const parsed = parsePortAndProtocol(portMatch?.[1] ?? directTarget);
  const protocol = normalizeProtocol(protoMatch?.[1] ?? portMatch?.[2]) ?? parsed.protocol;

  return {
    id: `ufw:added:${index}`,
    action: normalizeAction(actionText),
    protocol,
    port: parsed.port,
    source: sourceMatch?.[1]?.trim() || undefined,
    target: targetMatch?.[1]?.trim() || directTarget || undefined,
    direction: routeText ? 'route' : directionMatch?.[1]?.toUpperCase(),
    raw: line,
  };
}

function parseUfwRules(stdout: string): FirewallRule[] {
  const statusLines: string[] = [];
  const addedLines: string[] = [];
  let inAddedSection = false;

  stdout.split(/\r?\n/).forEach((rawLine) => {
    const line = rawLine.trimEnd();

    if (!line.trim()) {
      return;
    }

    if (line.startsWith(ufwAddedMarker)) {
      inAddedSection = true;
      return;
    }

    if (inAddedSection) {
      addedLines.push(line);
    } else {
      statusLines.push(line);
    }
  });

  const statusRules = statusLines
    .map(parseUfwStatusRuleLine)
    .filter((rule): rule is FirewallRule => Boolean(rule));

  if (statusRules.length) {
    return statusRules;
  }

  return addedLines
    .map(parseUfwAddedRuleLine)
    .filter((rule): rule is FirewallRule => Boolean(rule));
}

function parseFirewalldRules(stdout: string): FirewallRule[] {
  const rules: FirewallRule[] = [];
  const portsLine = stdout.split(/\r?\n/).find((line) => /^\s*ports:\s*/i.test(line));
  const servicesLine = stdout.split(/\r?\n/).find((line) => /^\s*services:\s*/i.test(line));
  const richRules = stdout.split(/\r?\n/).filter((line) => /^\s*rule\s+/i.test(line.trim()));

  const portsText = portsLine?.replace(/^\s*ports:\s*/i, '').trim() ?? '';
  portsText.split(/\s+/).filter(Boolean).forEach((portText, index) => {
    const parsed = parsePortAndProtocol(portText);
    rules.push({
      id: `firewalld:port:${portText}:${index}`,
      action: 'allow',
      protocol: parsed.protocol,
      port: parsed.port,
      target: portText,
      raw: portText,
    });
  });

  const servicesText = servicesLine?.replace(/^\s*services:\s*/i, '').trim() ?? '';
  servicesText.split(/\s+/).filter(Boolean).forEach((service, index) => {
    rules.push({
      id: `firewalld:service:${service}:${index}`,
      action: 'allow',
      target: service,
      raw: `service ${service}`,
    });
  });

  richRules.forEach((line, index) => {
    rules.push({
      id: `firewalld:rich:${index}`,
      action: normalizeAction(line),
      raw: line.trim(),
    });
  });

  return rules;
}

function parseWindowsSnapshot(stdout: string, stderr: string): FirewallSnapshot {
  const parsed = JSON.parse(stdout.trim() || '{}') as Record<string, unknown>;
  const profiles = toRecords(parsed.profiles);
  const enabledProfiles = profiles.filter((profile) => /true/i.test(readString(profile, 'Enabled')));
  const profileLabel = profiles.map((profile) => {
    const name = readString(profile, 'Name') || 'Profile';
    const enabled = /true/i.test(readString(profile, 'Enabled')) ? '启用' : '关闭';
    const inbound = readString(profile, 'DefaultInboundAction') || '-';
    const outbound = readString(profile, 'DefaultOutboundAction') || '-';
    return `${name}: ${enabled}, 入站 ${inbound}, 出站 ${outbound}`;
  }).join('\n');
  const rules = toRecords(parsed.rules).map<FirewallRule>((record, index) => {
    const name = readString(record, 'Name') || `rule-${index}`;
    const displayName = readString(record, 'DisplayName') || name;
    const protocol = normalizeProtocol(readString(record, 'Protocol'));
    const port = readString(record, 'LocalPort') || undefined;
    const source = readString(record, 'RemoteAddress') || undefined;
    const action = normalizeAction(readString(record, 'Action'));
    const direction = readString(record, 'Direction') || undefined;

    return {
      id: `windows:${name}`,
      action,
      protocol,
      port,
      source,
      target: displayName,
      direction,
      raw: [displayName, direction, action, protocol, port, source].filter(Boolean).join(' | '),
    };
  });

  return {
    backend: 'windows',
    status: enabledProfiles.length ? `已启用 ${enabledProfiles.length}/${profiles.length} 个配置文件` : '未启用',
    defaultPolicy: profileLabel || '未读取到配置文件策略',
    rules,
    rawOutput: [stdout, stderr].filter(Boolean).join('\n'),
  };
}

export function createFirewallStatusCommand(isWindowsHost: boolean) {
  if (isWindowsHost) {
    return powershellCommand(`
$profiles = Get-NetFirewallProfile | Select-Object Name,Enabled,DefaultInboundAction,DefaultOutboundAction
$rules = Get-NetFirewallRule -PolicyStore ActiveStore -Enabled True -ErrorAction SilentlyContinue | Select-Object -First 800 | ForEach-Object {
  $portFilter = Get-NetFirewallPortFilter -AssociatedNetFirewallRule $_ -ErrorAction SilentlyContinue | Select-Object -First 1
  $addressFilter = Get-NetFirewallAddressFilter -AssociatedNetFirewallRule $_ -ErrorAction SilentlyContinue | Select-Object -First 1
  [pscustomobject]@{
    Name = [string]$_.Name
    DisplayName = [string]$_.DisplayName
    Direction = [string]$_.Direction
    Action = [string]$_.Action
    Protocol = if ($portFilter) { [string]$portFilter.Protocol } else { "Any" }
    LocalPort = if ($portFilter) { [string]$portFilter.LocalPort } else { "" }
    RemoteAddress = if ($addressFilter) { [string]$addressFilter.RemoteAddress } else { "" }
  }
}
[pscustomobject]@{ profiles = @($profiles); rules = @($rules) } | ConvertTo-Json -Depth 6 -Compress
`);
  }

  return `
if command -v ufw >/dev/null 2>&1; then
  printf '${backendMarker}\\tufw\\n'
  if command -v sudo >/dev/null 2>&1 && sudo -n true >/dev/null 2>&1; then
    sudo -n ufw status numbered verbose 2>/dev/null || sudo -n ufw status numbered 2>/dev/null || sudo -n ufw status verbose 2>/dev/null || sudo -n ufw status 2>/dev/null
  else
    ufw status numbered verbose 2>/dev/null || ufw status numbered 2>/dev/null || ufw status verbose 2>/dev/null || ufw status 2>/dev/null
  fi
  printf '${ufwAddedMarker}\\n'
  if command -v sudo >/dev/null 2>&1 && sudo -n true >/dev/null 2>&1; then
    sudo -n ufw show added 2>/dev/null || true
  else
    ufw show added 2>/dev/null || true
  fi
  exit 0
fi
if command -v firewall-cmd >/dev/null 2>&1; then
  printf '${backendMarker}\\tfirewalld\\n'
  printf '${stateMarker}\\n'
  state=$(sudo -n firewall-cmd --state 2>/dev/null || firewall-cmd --state 2>&1 || printf unknown)
  printf '%s\\n' "$state"
  zone=$(sudo -n firewall-cmd --get-default-zone 2>/dev/null || firewall-cmd --get-default-zone 2>/dev/null || sudo -n firewall-cmd --permanent --get-default-zone 2>/dev/null || firewall-cmd --permanent --get-default-zone 2>/dev/null || printf public)
  printf '${zoneMarker}\\t%s\\n' "$zone"
  printf '${listMarker}\\n'
  if printf '%s' "$state" | grep -qi '^running$'; then
    sudo -n firewall-cmd --list-all --zone="$zone" 2>&1 || firewall-cmd --list-all --zone="$zone" 2>&1 || sudo -n firewall-cmd --permanent --list-all --zone="$zone" 2>&1 || firewall-cmd --permanent --list-all --zone="$zone" 2>&1
  else
    sudo -n firewall-cmd --permanent --list-all --zone="$zone" 2>&1 || firewall-cmd --permanent --list-all --zone="$zone" 2>&1 || sudo -n firewall-cmd --list-all --zone="$zone" 2>&1 || firewall-cmd --list-all --zone="$zone" 2>&1
  fi
  exit 0
fi
printf '${backendMarker}\\tunknown\\n'
printf '未检测到 ufw 或 firewalld。\\n'
`;
}

export function parseFirewallSnapshot(stdout: string, stderr: string, isWindowsHost: boolean): FirewallSnapshot {
  if (isWindowsHost) {
    return parseWindowsSnapshot(stdout, stderr);
  }

  const backendLine = stdout.split(/\r?\n/).find((line) => line.startsWith(backendMarker));
  const backend = (backendLine?.split('\t')[1] as FirewallBackend | undefined) ?? 'unknown';
  const rawOutput = [stdout, stderr].filter(Boolean).join('\n');

  if (backend === 'ufw') {
    const status = stdout.match(/^(?:Status|状态):\s*(.+)$/im)?.[1]?.trim() ?? '未知';
    const defaultPolicy = stdout.match(/^(?:Default|默认):\s*(.+)$/im)?.[1]?.trim() ?? '未读取到默认策略';

    return {
      backend,
      status,
      defaultPolicy,
      rules: parseUfwRules(stdout),
      rawOutput,
    };
  }

  if (backend === 'firewalld') {
    const zone = stdout.match(new RegExp(`^${zoneMarker}\\t(.+)$`, 'm'))?.[1]?.trim() || 'public';
    const stateMatch = stdout.match(new RegExp(`${stateMarker}\\r?\\n([^\\r\\n]+)`, 'm'));
    const status = stateMatch?.[1]?.trim() || '未知';
    const target = stdout.match(/^\s*target:\s*(.+)$/im)?.[1]?.trim();

    return {
      backend,
      status,
      defaultPolicy: `zone ${zone}${target ? `, target ${target}` : ''}`,
      zone,
      rules: parseFirewalldRules(stdout),
      rawOutput,
    };
  }

  return {
    backend: 'unknown',
    status: '未检测到支持的防火墙工具',
    defaultPolicy: '可安装 ufw 或 firewalld 后刷新',
    rules: [],
    rawOutput,
  };
}

export function validateFirewallDraft(draft: FirewallRuleDraft, backend: FirewallBackend) {
  const port = draft.port.trim();
  const source = draft.source.trim();
  const portMatch = port.match(/^(\d{1,5})(?:[:-](\d{1,5}))?$/);

  if (!portMatch) {
    throw new Error('端口必须是 1-65535，或端口范围，例如 8000-8010。');
  }

  const startPort = Number.parseInt(portMatch[1], 10);
  const endPort = portMatch[2] ? Number.parseInt(portMatch[2], 10) : startPort;

  if (startPort < 1 || endPort > 65535 || startPort > endPort) {
    throw new Error('端口范围必须位于 1-65535 内。');
  }

  if (source && source.length > 120) {
    throw new Error('来源地址过长。');
  }

  if (source && /[\r\n;&|`$<>]/.test(source)) {
    throw new Error('来源地址包含不安全字符。');
  }

  if (backend === 'firewalld' && draft.action !== 'allow') {
    throw new Error('firewalld 首版仅支持新增允许端口。');
  }
}

export function isRiskyFirewallDraft(draft: FirewallRuleDraft) {
  const port = draft.port.trim();
  const source = draft.source.trim();
  const riskyPorts = new Set(['22', '3389', '3306', '5432', '6379', '9200', '9300', '11211', '27017']);
  const openSource = !source || source === '0.0.0.0/0' || /^any(where)?$/i.test(source);

  return openSource || riskyPorts.has(port);
}

function parsePortRange(value?: string) {
  const normalized = String(value ?? '').trim().toLowerCase()
    .replace(/\s+\(v6\)$/i, '')
    .replace(/\/(?:tcp|udp|any)$/i, '')
    .replace(':', '-');
  const match = normalized.match(/^(\d{1,5})(?:-(\d{1,5}))?$/);

  if (!match) {
    return null;
  }

  const start = Number.parseInt(match[1], 10);
  const end = match[2] ? Number.parseInt(match[2], 10) : start;

  if (start < 1 || end > 65535 || start > end) {
    return null;
  }

  return { start, end };
}

function portMatches(value: string | undefined, port: number) {
  const normalized = String(value ?? '').trim().toLowerCase();

  if (!normalized) {
    return false;
  }

  if ((normalized === 'ssh' || normalized === 'openssh') && port === 22) {
    return true;
  }

  const range = parsePortRange(normalized);
  return Boolean(range && port >= range.start && port <= range.end);
}

function rawRuleMatchesSshPort(raw: string, port: number) {
  const normalized = raw.toLowerCase();

  if (port === 22 && /\bservice\s+(?:name=)?["']?(?:ssh|openssh)(?:["']|\b)/.test(normalized)) {
    return true;
  }

  const portMatch = normalized.match(/\bport\s+(?:port=)?["']?(\d{1,5}(?:[-:]\d{1,5})?)["']?/);
  return portMatches(portMatch?.[1], port);
}

function defaultPolicyAllowsInbound(snapshot: FirewallSnapshot) {
  const policy = snapshot.defaultPolicy.toLowerCase();

  if (snapshot.backend === 'ufw') {
    return /allow\s*\([^)]*incoming|incoming[^,;]*allow|允许[^,;]*(?:入站|进入)|(?:入站|进入)[^,;]*允许/i.test(snapshot.defaultPolicy);
  }

  if (snapshot.backend === 'firewalld') {
    return /\btarget\s+accept\b/i.test(policy);
  }

  return false;
}

export function isFirewallEnabled(snapshot: FirewallSnapshot) {
  const status = snapshot.status.trim().toLowerCase();

  if (snapshot.backend === 'ufw') {
    if (/^(?:inactive|disabled|not\s+active|not\s+enabled)\b/.test(status) || /不活动|未启用|停用|关闭/.test(status)) {
      return false;
    }

    return /^(?:active|enabled)\b/.test(status) || /\b(?:active|enabled)\b/.test(status) || /活动|已启用|启用中/.test(status);
  }

  if (snapshot.backend === 'firewalld') {
    if (/\bnot\s+running\b|inactive|dead|stopped|未运行|停止|停用|关闭/.test(status)) {
      return false;
    }

    return /^running\b/.test(status) || /\brunning\b/.test(status) || /运行中|已运行/.test(status);
  }

  return false;
}

export function isFirewallSshPortAllowed(snapshot: FirewallSnapshot, sshPort: number) {
  if (!Number.isInteger(sshPort) || sshPort < 1 || sshPort > 65535) {
    return false;
  }

  if (defaultPolicyAllowsInbound(snapshot)) {
    return true;
  }

  return snapshot.rules.some((rule) => {
    if (rule.action !== 'allow' && rule.action !== 'limit') {
      return false;
    }

    if (rule.protocol && rule.protocol !== 'tcp' && rule.protocol !== 'any') {
      return false;
    }

    return portMatches(rule.port, sshPort)
      || portMatches(rule.target, sshPort)
      || rawRuleMatchesSshPort(rule.raw, sshPort);
  });
}

export function createFirewallAddRuleCommand(backend: FirewallBackend, draft: FirewallRuleDraft, zone?: string) {
  validateFirewallDraft(draft, backend);
  const port = draft.port.trim().replace(':', '-');
  const source = draft.source.trim();
  const protocol = draft.protocol === 'any' ? 'tcp' : draft.protocol;

  if (backend === 'ufw') {
    const protoSuffix = draft.protocol === 'any' ? '' : ` proto ${draft.protocol}`;
    const sourcePart = source && !/^any(where)?$/i.test(source) ? `from ${shellSingleQuote(source)} to any port ${shellSingleQuote(port)}${protoSuffix}` : `${shellSingleQuote(`${port}/${protocol}`)}`;
    const actionPart = draft.action === 'deny' || draft.action === 'reject'
      ? `prepend ${draft.action}`
      : draft.action;
    return `sudo -n ufw ${actionPart} ${sourcePart}`;
  }

  if (backend === 'firewalld') {
    const targetZone = zone || 'public';
    return `sudo -n firewall-cmd --zone=${shellSingleQuote(targetZone)} --add-port=${shellSingleQuote(`${port}/${protocol}`)} --permanent && sudo -n firewall-cmd --reload`;
  }

  if (backend === 'windows') {
    const action = draft.action === 'allow' ? 'Allow' : 'Block';
    const remoteAddress = source && !/^any(where)?$/i.test(source) ? source : 'Any';
    return powershellCommand(`
New-NetFirewallRule -DisplayName ${powershellSingleQuote(`ShellDesk ${draft.action} ${protocol} ${port}`)} -Direction Inbound -Action ${action} -Protocol ${protocol.toUpperCase()} -LocalPort ${powershellSingleQuote(port)} -RemoteAddress ${powershellSingleQuote(remoteAddress)}
`);
  }

  throw new Error('未检测到可操作的防火墙后端。');
}

function normalizeUfwAddress(value?: string) {
  return String(value ?? '').trim().replace(/\s+\(v6\)$/i, '');
}

function isUfwAnyAddress(value?: string) {
  const normalized = normalizeUfwAddress(value).toLowerCase();
  return !normalized || normalized === 'any' || normalized === 'anywhere' || normalized === '0.0.0.0/0' || normalized === '::/0';
}

function getUfwActionToken(action: FirewallAction) {
  if (action === 'allow' || action === 'deny' || action === 'reject' || action === 'limit') {
    return action;
  }

  throw new Error('该 ufw 规则动作无法识别，不能安全删除。');
}

function isUfwPortTarget(target: string, port?: string, protocol?: FirewallProtocol) {
  const normalizedTarget = normalizeUfwAddress(target);
  const normalizedPort = String(port ?? '').trim();

  if (!normalizedTarget || !normalizedPort) {
    return false;
  }

  return normalizedTarget === normalizedPort || normalizedTarget === `${normalizedPort}/${protocol ?? ''}`;
}

function createUfwRuleDeleteSpec(rule: FirewallRule) {
  const action = getUfwActionToken(rule.action);
  const parts = [];
  const direction = String(rule.direction ?? '').trim().toLowerCase();
  const source = normalizeUfwAddress(rule.source);
  const target = normalizeUfwAddress(rule.target);
  const port = String(rule.port ?? '').trim();
  const protocol = rule.protocol && rule.protocol !== 'any' ? rule.protocol : '';

  if (direction === 'route') {
    parts.push('route');
  }

  parts.push(action);

  if (!isUfwAnyAddress(source)) {
    const destination = target && !isUfwPortTarget(target, port, rule.protocol) ? target : 'any';
    parts.push('from', shellSingleQuote(source), 'to', shellSingleQuote(destination));

    if (port) {
      parts.push('port', shellSingleQuote(port));
    }

    if (protocol) {
      parts.push('proto', protocol);
    }

    return parts.join(' ');
  }

  if (port) {
    parts.push(shellSingleQuote(`${port}${protocol ? `/${protocol}` : ''}`));
    return parts.join(' ');
  }

  if (target) {
    parts.push(shellSingleQuote(target));
    return parts.join(' ');
  }

  throw new Error('该 ufw 规则缺少可删除的端口或目标。');
}

export function createFirewallDeleteRuleCommand(backend: FirewallBackend, rule: FirewallRule, zone?: string) {
  if (backend === 'ufw') {
    const number = rule.id.match(/^ufw:(\d+)$/)?.[1];

    return number
      ? `sudo -n ufw --force delete ${number}`
      : `sudo -n ufw --force delete ${createUfwRuleDeleteSpec(rule)}`;
  }

  if (backend === 'firewalld') {
    if (!rule.port || !rule.protocol || rule.protocol === 'any') {
      throw new Error('首版仅支持删除 firewalld 端口规则。');
    }

    const targetZone = zone || 'public';
    return `sudo -n firewall-cmd --zone=${shellSingleQuote(targetZone)} --remove-port=${shellSingleQuote(`${rule.port}/${rule.protocol}`)} --permanent && sudo -n firewall-cmd --reload`;
  }

  if (backend === 'windows') {
    const name = rule.id.replace(/^windows:/, '');
    return powershellCommand(`Remove-NetFirewallRule -Name ${powershellSingleQuote(name)}`);
  }

  throw new Error('未检测到可操作的防火墙后端。');
}

export function createFirewallReloadCommand(backend: FirewallBackend) {
  if (backend === 'ufw') return 'sudo -n ufw reload';
  if (backend === 'firewalld') return 'sudo -n firewall-cmd --reload';
  if (backend === 'windows') return powershellCommand('Get-NetFirewallProfile | Format-Table -AutoSize | Out-String');
  throw new Error('未检测到可重载的防火墙后端。');
}

export function createFirewallSetEnabledCommand(backend: FirewallBackend, enabled: boolean) {
  if (backend === 'ufw') {
    return enabled ? 'sudo -n ufw --force enable' : 'sudo -n ufw disable';
  }

  if (backend === 'firewalld') {
    return enabled
      ? `if command -v systemctl >/dev/null 2>&1; then
  sudo -n systemctl enable --now firewalld
elif command -v service >/dev/null 2>&1; then
  sudo -n service firewalld start
else
  printf '未检测到 systemctl 或 service，无法启用 firewalld。\\n' >&2
  exit 1
fi`
      : `if command -v systemctl >/dev/null 2>&1; then
  sudo -n systemctl disable --now firewalld
elif command -v service >/dev/null 2>&1; then
  sudo -n service firewalld stop
else
  printf '未检测到 systemctl 或 service，无法停用 firewalld。\\n' >&2
  exit 1
fi`;
  }

  throw new Error('启用/停用仅支持 ufw 或 firewalld 后端。');
}

export function getFirewallBackendLabel(backend: FirewallBackend) {
  if (backend === 'ufw') return 'ufw';
  if (backend === 'firewalld') return 'firewalld';
  if (backend === 'windows') return 'Windows Firewall';
  return '未识别';
}

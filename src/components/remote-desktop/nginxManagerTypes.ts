export type NginxDistro = 'debian' | 'rhel' | 'alpine' | 'unknown';
export type NginxSitesLayout = 'debian' | 'rhel';
export type NginxSiteFilter = 'all' | 'enabled' | 'disabled' | 'ssl' | 'non-ssl';

export interface NginxInstallation {
  version: string;
  modules: string[];
  configPath: string;
  configDir: string;
  errorLogPath: string;
  pidFile: string;
  binaryPath: string;
  distro: NginxDistro;
  sitesLayout: NginxSitesLayout;
  availableDir: string | null;
  enabledDir: string | null;
  confDir: string;
  logDir: string;
  isRunning: boolean;
}

export interface NginxConfigFile {
  filename: string;
  fullPath: string;
  isEnabled: boolean;
  enabledPath: string | null;
  serverBlocks: NginxServerBlock[];
  upstreamBlocks: NginxUpstreamBlock[];
  rawContent: string;
  lastModified: number;
  fileSize: number;
}

export interface NginxServerBlock {
  id: string;
  configPath: string;
  startLine: number;
  endLine: number;
  serverNames: string[];
  listenDirectives: NginxListenDirective[];
  locations: NginxLocationBlock[];
  sslConfig: NginxSslConfig | null;
  root: string | null;
  index: string | null;
  accessLog: string | null;
  errorLog: string | null;
  rawDirectives: NginxDirective[];
}

export interface NginxListenDirective {
  address: string;
  port: number;
  ssl: boolean;
  http2: boolean;
  defaultServer: boolean;
  raw: string;
}

export interface NginxLocationBlock {
  id: string;
  modifier: '' | '=' | '~' | '~*' | '^~' | '@';
  path: string;
  proxyPass: string | null;
  fastcgiPass: string | null;
  root: string | null;
  alias: string | null;
  tryFiles: string[] | null;
  rawDirectives: NginxDirective[];
  nestedLocations: NginxLocationBlock[];
  startLine: number;
  endLine: number;
}

export interface NginxUpstreamBlock {
  id: string;
  name: string;
  method: string;
  servers: NginxUpstreamServer[];
  keepalive: number | null;
  rawDirectives: NginxDirective[];
  configPath: string;
  startLine: number;
}

export interface NginxUpstreamServer {
  address: string;
  weight: number | null;
  maxFails: number | null;
  failTimeout: string | null;
  backup: boolean;
  down: boolean;
  raw: string;
}

export interface NginxSslConfig {
  certificate: string;
  certificateKey: string;
  protocols: string[];
  ciphers: string | null;
  hsts: boolean;
}

export interface NginxDirective {
  name: string;
  params: string[];
  line: number;
}

export interface NginxTestResult {
  success: boolean;
  output: string;
  errors: NginxTestError[];
}

export interface NginxTestError {
  file: string;
  line: number;
  message: string;
}

export interface NginxTemplateVariable {
  name: string;
  label: string;
  type: 'text' | 'number' | 'port' | 'path' | 'select' | 'boolean';
  default: string;
  required: boolean;
  description: string;
  options?: string[];
}

export interface NginxConfigTemplate {
  id: string;
  name: string;
  description: string;
  category: 'static' | 'proxy' | 'php' | 'ssl' | 'loadbalancer' | 'websocket';
  icon: string;
  variables: NginxTemplateVariable[];
  content: string;
}

export type CaddySiteFilter = 'all' | 'enabled' | 'disabled' | 'tls' | 'non-tls';

export interface CaddyInstallation {
  version: string;
  configPath: string;
  configDir: string;
  isAdminApiEnabled: boolean;
  adminApiUrl: string;
  isRunning: boolean;
  distro: 'debian' | 'rhel' | 'alpine' | 'unknown';
}

export interface CaddySiteBlock {
  id: string;
  matcher: string;
  listen: string[];
  tls: boolean;
  directives: CaddyDirective[];
  filePath: string;
  startLine: number;
  endLine: number;
}

export interface CaddyDirective {
  name: string;
  args: string[];
  block: CaddyDirective[] | null;
  line: number;
}

export interface CaddyConfigFile {
  filename: string;
  fullPath: string;
  rawContent: string;
  siteBlocks: CaddySiteBlock[];
  globalDirectives: CaddyDirective[];
  lastModified: number;
  fileSize: number;
}

export interface CaddyTestResult {
  success: boolean;
  output: string;
  errors: CaddyTestError[];
}

export interface CaddyTestError {
  file: string;
  line: number;
  message: string;
}

export interface CaddyTemplateVariable {
  name: string;
  label: string;
  type: 'text' | 'number' | 'port' | 'path' | 'select' | 'boolean';
  default: string;
  required: boolean;
  description: string;
  options?: string[];
}

export interface CaddyConfigTemplate {
  id: string;
  name: string;
  description: string;
  category: 'static' | 'reverse-proxy' | 'php' | 'api' | 'file-server' | 'docker';
  icon: string;
  variables: CaddyTemplateVariable[];
  content: string;
}

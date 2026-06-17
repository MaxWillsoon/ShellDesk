import type { CaddyConfigTemplate, CaddyTemplateVariable } from './caddyManagerTypes';
import { tCurrent, type MessageId } from '../../i18n';

export const caddyConfigTemplates: CaddyConfigTemplate[] = [
  {
    id: 'static',
    name: 'caddy.templates.static.name',
    description: 'caddy.templates.static.description',
    category: 'static',
    icon: 'FileText',
    variables: [
      { name: 'DOMAIN', label: 'caddy.variables.domain', type: 'text', default: 'example.com', required: true, description: 'caddy.variables.domain.description' },
      { name: 'ROOT_PATH', label: 'caddy.variables.rootPath', type: 'path', default: '/var/www/example.com', required: true, description: 'caddy.variables.rootPath.description' },
    ],
    content: `{{DOMAIN}} {
    root * {{ROOT_PATH}}
    file_server
}`,
  },
  {
    id: 'reverse-proxy',
    name: 'caddy.templates.reverseProxy.name',
    description: 'caddy.templates.reverseProxy.description',
    category: 'reverse-proxy',
    icon: 'Shuffle',
    variables: [
      { name: 'DOMAIN', label: 'caddy.variables.domain', type: 'text', default: 'app.example.com', required: true, description: 'caddy.variables.domain.description' },
      { name: 'UPSTREAM_HOST', label: 'caddy.variables.upstreamHost', type: 'text', default: '127.0.0.1', required: true, description: 'caddy.variables.upstreamHost.description' },
      { name: 'UPSTREAM_PORT', label: 'caddy.variables.upstreamPort', type: 'port', default: '3000', required: true, description: 'caddy.variables.upstreamPort.description' },
    ],
    content: `{{DOMAIN}} {
    reverse_proxy {{UPSTREAM_HOST}}:{{UPSTREAM_PORT}}
}`,
  },
  {
    id: 'php',
    name: 'caddy.templates.php.name',
    description: 'caddy.templates.php.description',
    category: 'php',
    icon: 'Code2',
    variables: [
      { name: 'DOMAIN', label: 'caddy.variables.domain', type: 'text', default: 'wordpress.example.com', required: true, description: 'caddy.variables.domain.description' },
      { name: 'ROOT_PATH', label: 'caddy.variables.rootPath', type: 'path', default: '/var/www/wordpress', required: true, description: 'caddy.variables.rootPath.description' },
      { name: 'FASTCGI_ADDRESS', label: 'caddy.variables.fastcgiAddress', type: 'text', default: 'unix//run/php/php8.2-fpm.sock', required: true, description: 'caddy.variables.fastcgiAddress.description' },
    ],
    content: `{{DOMAIN}} {
    root * {{ROOT_PATH}}
    php_fastcgi {{FASTCGI_ADDRESS}}
    file_server
}`,
  },
  {
    id: 'tls',
    name: 'caddy.templates.tls.name',
    description: 'caddy.templates.tls.description',
    category: 'static',
    icon: 'ShieldCheck',
    variables: [
      { name: 'DOMAIN', label: 'caddy.variables.domain', type: 'text', default: 'secure.example.com', required: true, description: 'caddy.variables.domain.description' },
      { name: 'EMAIL', label: 'caddy.variables.email', type: 'text', default: 'admin@example.com', required: true, description: 'caddy.variables.email.description' },
      { name: 'ROOT_PATH', label: 'caddy.variables.rootPath', type: 'path', default: '/var/www/secure.example.com', required: true, description: 'caddy.variables.rootPath.description' },
    ],
    content: `{{DOMAIN}} {
    tls {{EMAIL}}
    root * {{ROOT_PATH}}
    file_server
}`,
  },
  {
    id: 'api',
    name: 'caddy.templates.api.name',
    description: 'caddy.templates.api.description',
    category: 'api',
    icon: 'Network',
    variables: [
      { name: 'DOMAIN', label: 'caddy.variables.domain', type: 'text', default: 'api.example.com', required: true, description: 'caddy.variables.domain.description' },
      { name: 'API_UPSTREAM', label: 'caddy.variables.apiUpstream', type: 'text', default: '127.0.0.1:4000', required: true, description: 'caddy.variables.apiUpstream.description' },
      { name: 'FRONTEND_PATH', label: 'caddy.variables.frontendPath', type: 'path', default: '/var/www/app', required: true, description: 'caddy.variables.frontendPath.description' },
    ],
    content: `{{DOMAIN}} {
    handle /api/* {
        reverse_proxy {{API_UPSTREAM}}
    }
    handle {
        root * {{FRONTEND_PATH}}
        file_server
    }
}`,
  },
  {
    id: 'docker',
    name: 'caddy.templates.docker.name',
    description: 'caddy.templates.docker.description',
    category: 'docker',
    icon: 'Container',
    variables: [
      { name: 'DOMAIN', label: 'caddy.variables.domain', type: 'text', default: 'container.example.com', required: true, description: 'caddy.variables.domain.description' },
      { name: 'CONTAINER_NAME', label: 'caddy.variables.containerName', type: 'text', default: 'app', required: true, description: 'caddy.variables.containerName.description' },
      { name: 'CONTAINER_PORT', label: 'caddy.variables.containerPort', type: 'port', default: '8080', required: true, description: 'caddy.variables.containerPort.description' },
    ],
    content: `{{DOMAIN}} {
    reverse_proxy {{CONTAINER_NAME}}:{{CONTAINER_PORT}}
    encode gzip
    header {
        X-Content-Type-Options nosniff
        X-Frame-Options SAMEORIGIN
    }
}`,
  },
];

const domainPattern = /^(?:\*\.)?(?:[a-z0-9-]+\.)+[a-z]{2,}$/i;
const hostPattern = /^(?:localhost|(?:[a-z0-9-]+\.)*[a-z0-9-]+|\d{1,3}(?:\.\d{1,3}){3}|\[[0-9a-f:]+\])$/i;
const addressPattern = /^(?:https?:\/\/)?(?:localhost|(?:[a-z0-9-]+\.)*[a-z0-9-]+|\d{1,3}(?:\.\d{1,3}){3}|\[[0-9a-f:]+\])(?::\d{1,5})?(?:\/[^\s{}]*)?$/i;

function templateError(id: MessageId, name: string) {
  return tCurrent(id, { value0: name });
}

export function sanitizeCaddyTemplateValue(name: string, type: CaddyTemplateVariable['type'], value: string): string {
  const stripped = value.trim();
  const upperName = name.toUpperCase();
  if (!stripped) throw new Error(templateError('caddy.templateErrors.required', name));
  if (/[\r\n{}]/.test(stripped)) throw new Error(templateError('caddy.templateErrors.unsupportedChars', name));
  if (type === 'port') {
    if (!/^\d+$/.test(stripped)) throw new Error(templateError('caddy.templateErrors.numericPort', name));
    const port = Number(stripped);
    if (port < 1 || port > 65535) throw new Error(templateError('caddy.templateErrors.portRange', name));
  }
  if (type === 'path' && (!stripped.startsWith('/') || /\s/.test(stripped))) throw new Error(templateError('caddy.templateErrors.path', name));
  if (type === 'number' && !/^\d+$/.test(stripped)) throw new Error(templateError('caddy.templateErrors.numeric', name));
  if (upperName.includes('DOMAIN')) {
    if (!domainPattern.test(stripped)) throw new Error(templateError('caddy.templateErrors.domain', name));
  }
  if ((upperName.includes('HOST') || upperName.includes('CONTAINER_NAME')) && (/\s/.test(stripped) || !hostPattern.test(stripped))) throw new Error(templateError('caddy.templateErrors.host', name));
  if (upperName.includes('ADDRESS') && !(/^unix\/\//.test(stripped) || addressPattern.test(stripped))) throw new Error(templateError('caddy.templateErrors.address', name));
  if (upperName.includes('UPSTREAM') && !addressPattern.test(stripped)) throw new Error(templateError('caddy.templateErrors.address', name));
  if (upperName.includes('EMAIL') && !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(stripped)) throw new Error(templateError('caddy.templateErrors.email', name));
  return stripped;
}

export function renderCaddyTemplate(template: CaddyConfigTemplate, values: Record<string, string>): string {
  const defaults = new Map(template.variables.map((variable) => [variable.name, variable.default]));
  const variables = new Map(template.variables.map((variable) => [variable.name, variable]));
  return template.content.replace(/\{\{([A-Z0-9_]+)(?:\|default:([^}]+))?\}\}/g, (_match, name: string, inlineDefault?: string) => (
    sanitizeCaddyTemplateValue(name, variables.get(name)?.type ?? 'text', values[name]?.trim() || inlineDefault || defaults.get(name) || '')
  ));
}

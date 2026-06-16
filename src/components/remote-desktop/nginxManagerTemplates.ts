import type { NginxConfigTemplate } from './nginxManagerTypes';

export const nginxConfigTemplates: NginxConfigTemplate[] = [
  {
    id: 'static',
    name: 'nginx.templates.static.name',
    description: 'nginx.templates.static.description',
    category: 'static',
    icon: 'FileText',
    variables: [
      { name: 'SERVER_NAME', label: 'nginx.variables.serverName', type: 'text', default: 'example.com www.example.com', required: true, description: 'nginx.variables.serverName.description' },
      { name: 'ROOT_PATH', label: 'nginx.variables.rootPath', type: 'path', default: '/var/www/example.com', required: true, description: 'nginx.variables.rootPath.description' },
      { name: 'ACCESS_LOG', label: 'nginx.variables.accessLog', type: 'path', default: '/var/log/nginx/example.com.access.log', required: false, description: 'nginx.variables.accessLog.description' },
    ],
    content: `server {
    listen 80;
    listen [::]:80;
    server_name {{SERVER_NAME}};

    root {{ROOT_PATH}};
    index index.html index.htm;

    access_log {{ACCESS_LOG|default:/var/log/nginx/access.log}};

    location / {
        try_files $uri $uri/ =404;
    }

    location ~* \\.(?:css|js|jpg|jpeg|gif|png|svg|ico|webp|woff2?)$ {
        expires 30d;
        add_header Cache-Control "public, immutable";
        try_files $uri =404;
    }
}`,
  },
  {
    id: 'proxy',
    name: 'nginx.templates.proxy.name',
    description: 'nginx.templates.proxy.description',
    category: 'proxy',
    icon: 'Shuffle',
    variables: [
      { name: 'SERVER_NAME', label: 'nginx.variables.serverName', type: 'text', default: 'app.example.com', required: true, description: 'nginx.variables.serverName.description' },
      { name: 'UPSTREAM_URL', label: 'nginx.variables.upstreamUrl', type: 'text', default: 'http://127.0.0.1:3000', required: true, description: 'nginx.variables.upstreamUrl.description' },
      { name: 'TIMEOUT', label: 'nginx.variables.timeout', type: 'number', default: '60', required: true, description: 'nginx.variables.timeout.description' },
    ],
    content: `server {
    listen 80;
    server_name {{SERVER_NAME}};

    location / {
        proxy_pass {{UPSTREAM_URL}};
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_connect_timeout {{TIMEOUT|default:60}}s;
        proxy_send_timeout {{TIMEOUT|default:60}}s;
        proxy_read_timeout {{TIMEOUT|default:60}}s;
    }
}`,
  },
  {
    id: 'php',
    name: 'nginx.templates.php.name',
    description: 'nginx.templates.php.description',
    category: 'php',
    icon: 'Code2',
    variables: [
      { name: 'SERVER_NAME', label: 'nginx.variables.serverName', type: 'text', default: 'wordpress.example.com', required: true, description: 'nginx.variables.serverName.description' },
      { name: 'ROOT_PATH', label: 'nginx.variables.rootPath', type: 'path', default: '/var/www/wordpress', required: true, description: 'nginx.variables.rootPath.description' },
      { name: 'FASTCGI_PASS', label: 'nginx.variables.fastcgiPass', type: 'text', default: 'unix:/run/php/php8.2-fpm.sock', required: true, description: 'nginx.variables.fastcgiPass.description' },
    ],
    content: `server {
    listen 80;
    server_name {{SERVER_NAME}};
    root {{ROOT_PATH}};
    index index.php index.html;

    location / {
        try_files $uri $uri/ /index.php?$args;
    }

    location ~ \\.php$ {
        include snippets/fastcgi-php.conf;
        fastcgi_pass {{FASTCGI_PASS}};
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
    }

    location ~ /\\. {
        deny all;
    }

    location ~* /(?:uploads|files)/.*\\.php$ {
        deny all;
    }
}`,
  },
  {
    id: 'ssl',
    name: 'nginx.templates.ssl.name',
    description: 'nginx.templates.ssl.description',
    category: 'ssl',
    icon: 'ShieldCheck',
    variables: [
      { name: 'SERVER_NAME', label: 'nginx.variables.serverName', type: 'text', default: 'secure.example.com', required: true, description: 'nginx.variables.serverName.description' },
      { name: 'ROOT_PATH', label: 'nginx.variables.rootPath', type: 'path', default: '/var/www/secure.example.com', required: true, description: 'nginx.variables.rootPath.description' },
      { name: 'CERT_PATH', label: 'nginx.variables.certPath', type: 'path', default: '/etc/letsencrypt/live/secure.example.com/fullchain.pem', required: true, description: 'nginx.variables.certPath.description' },
      { name: 'KEY_PATH', label: 'nginx.variables.keyPath', type: 'path', default: '/etc/letsencrypt/live/secure.example.com/privkey.pem', required: true, description: 'nginx.variables.keyPath.description' },
    ],
    content: `server {
    listen 80;
    listen [::]:80;
    server_name {{SERVER_NAME}};
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name {{SERVER_NAME}};

    root {{ROOT_PATH}};
    index index.html index.htm;

    ssl_certificate {{CERT_PATH}};
    ssl_certificate_key {{KEY_PATH}};
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;
    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;

    location / {
        try_files $uri $uri/ =404;
    }
}`,
  },
  {
    id: 'loadbalancer',
    name: 'nginx.templates.loadbalancer.name',
    description: 'nginx.templates.loadbalancer.description',
    category: 'loadbalancer',
    icon: 'Network',
    variables: [
      { name: 'UPSTREAM_NAME', label: 'nginx.variables.upstreamName', type: 'text', default: 'backend_pool', required: true, description: 'nginx.variables.upstreamName.description' },
      { name: 'SERVER_NAME', label: 'nginx.variables.serverName', type: 'text', default: 'api.example.com', required: true, description: 'nginx.variables.serverName.description' },
      { name: 'BACKEND_1', label: 'nginx.variables.backend1', type: 'text', default: '127.0.0.1:3001', required: true, description: 'nginx.variables.backend.description' },
      { name: 'BACKEND_2', label: 'nginx.variables.backend2', type: 'text', default: '127.0.0.1:3002', required: true, description: 'nginx.variables.backend.description' },
      { name: 'METHOD', label: 'nginx.variables.balanceMethod', type: 'select', default: 'least_conn', required: false, description: 'nginx.variables.balanceMethod.description', options: ['least_conn', 'ip_hash', 'random'] },
    ],
    content: `upstream {{UPSTREAM_NAME}} {
    {{METHOD|default:least_conn}};
    server {{BACKEND_1}} max_fails=3 fail_timeout=30s;
    server {{BACKEND_2}} max_fails=3 fail_timeout=30s;
    keepalive 32;
}

server {
    listen 80;
    server_name {{SERVER_NAME}};

    location / {
        proxy_pass http://{{UPSTREAM_NAME}};
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}`,
  },
  {
    id: 'websocket',
    name: 'nginx.templates.websocket.name',
    description: 'nginx.templates.websocket.description',
    category: 'websocket',
    icon: 'Radio',
    variables: [
      { name: 'SERVER_NAME', label: 'nginx.variables.serverName', type: 'text', default: 'socket.example.com', required: true, description: 'nginx.variables.serverName.description' },
      { name: 'UPSTREAM_URL', label: 'nginx.variables.upstreamUrl', type: 'text', default: 'http://127.0.0.1:8080', required: true, description: 'nginx.variables.upstreamUrl.description' },
      { name: 'READ_TIMEOUT', label: 'nginx.variables.readTimeout', type: 'number', default: '3600', required: true, description: 'nginx.variables.readTimeout.description' },
    ],
    content: `server {
    listen 80;
    server_name {{SERVER_NAME}};

    location / {
        proxy_pass {{UPSTREAM_URL}};
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "Upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_read_timeout {{READ_TIMEOUT|default:3600}}s;
        proxy_send_timeout {{READ_TIMEOUT|default:3600}}s;
    }
}`,
  },
];

export function sanitizeNginxTemplateValue(name: string, value: string): string {
  const stripped = value.trim().replace(/[\r\n{};]/g, '');
  const upperName = name.toUpperCase();

  if (upperName.includes('PORT')) {
    if (!/^\d+$/.test(stripped)) throw new Error(`${name} must be a numeric port.`);
    const port = Number(stripped);
    if (port < 1 || port > 65535) throw new Error(`${name} must be between 1 and 65535.`);
  }

  if (upperName.includes('PATH') || upperName.endsWith('_LOG')) {
    if (!stripped.startsWith('/')) throw new Error(`${name} must start with /.`);
  }

  if (upperName.includes('URL')) {
    if (!/^https?:\/\//i.test(stripped)) throw new Error(`${name} must start with http:// or https://.`);
  }

  if (upperName.includes('SERVER_NAME')) {
    const domains = stripped.split(/\s+/).filter(Boolean);
    if (!domains.length || !domains.every((domain) => /^(\*\.)?([a-z0-9-]+\.)*[a-z0-9-]+(\.[a-z]{2,})$/i.test(domain))) {
      throw new Error(`${name} must contain valid domain names.`);
    }
  }

  return stripped;
}

export function renderNginxTemplate(template: NginxConfigTemplate, values: Record<string, string>): string {
  const defaults = new Map(template.variables.map((variable) => [variable.name, variable.default]));

  return template.content.replace(/\{\{([A-Z0-9_]+)(?:\|default:([^}]+))?\}\}/g, (_match, name: string, inlineDefault?: string) => {
    const value = values[name]?.trim();
    return sanitizeNginxTemplateValue(name, value || inlineDefault || defaults.get(name) || '');
  });
}

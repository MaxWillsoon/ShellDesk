const { app, ipcMain, session } = require('electron');
const crypto = require('node:crypto');
const {
  activeConnections,
  bindActiveConnectionClient,
  closeActiveConnection,
  connectSshClientWithJump,
  createSocksProxy,
  ensureActiveConnectionClient,
  findReusableActiveConnection,
  focusConnectionWindow,
  getActiveConnection,
  toConnectionInfo,
} = require('./connectionManager.cjs');
const { detectRemoteSystem } = require('./remoteConnectionHandlers.cjs');
const {
  createLocalClient,
  createLocalDisplayHost,
} = require('./localConnection.cjs');
const {
  classifyHostKey,
  describeHostKey,
  matchesHostAndPort,
  normalizeFingerprint,
  normalizeHostname,
} = require('./sshSecurity.cjs');
const { toConnectionErrorMessage, toErrorMessage } = require('./validation.cjs');
const { getVault, notifyVaultChanged, setVault, validateHostRequest } = require('./vaultStore.cjs');
const { createConnectionWindow } = require('./windows.cjs');

let isBrowserCertificateHandlerRegistered = false;
const sshPromptRequestTtlMs = 2 * 60 * 1000;
const keyboardInteractiveRequests = new Map();
const hostKeyVerificationRequests = new Map();

function createRequestId(prefix) {
  return `${prefix}-${crypto.randomUUID()}`;
}

function isWebContentsUsable(sender) {
  return Boolean(sender && typeof sender.isDestroyed === 'function' && !sender.isDestroyed());
}

function requestRendererDecision(sender, pendingRequests, channel, prefix, payload) {
  return new Promise((resolve) => {
    if (!isWebContentsUsable(sender)) {
      resolve({ cancel: true });
      return;
    }

    const requestId = createRequestId(prefix);
    const timeoutId = setTimeout(() => {
      settlePendingRequest(pendingRequests, { requestId }, { cancel: true, timeout: true });
    }, sshPromptRequestTtlMs);

    pendingRequests.set(requestId, {
      resolve,
      timeoutId,
      webContentsId: sender.id,
      createdAt: Date.now(),
    });

    try {
      sender.send(channel, {
        requestId,
        ...payload,
      });
    } catch {
      settlePendingRequest(pendingRequests, { requestId }, { cancel: true });
    }
  });
}

function settlePendingRequest(pendingRequests, rawPayload, response, event = null) {
  const requestId = String(rawPayload?.requestId || '');
  const pending = pendingRequests.get(requestId);

  if (!pending) {
    return { success: false, error: '请求已过期，请重试。' };
  }

  if (event?.sender?.id && pending.webContentsId && event.sender.id !== pending.webContentsId) {
    return { success: false, error: '请求来源不匹配。' };
  }

  clearTimeout(pending.timeoutId);
  pendingRequests.delete(requestId);
  pending.resolve(response);
  return { success: true };
}

function readPromptText(value) {
  return String(value || '').slice(0, 500);
}

function normalizeKeyboardPrompts(prompts) {
  return (Array.isArray(prompts) ? prompts : []).slice(0, 8).map((prompt) => ({
    prompt: readPromptText(prompt?.prompt),
    echo: Boolean(prompt?.echo),
  }));
}

function isPasswordKeyboardPrompt(prompt) {
  if (!prompt || prompt.echo) {
    return false;
  }

  const text = String(prompt.prompt || '').toLowerCase();

  if (!/(password|passcode|\u5bc6\u7801|\u53e3\u4ee4)/iu.test(text)) {
    return false;
  }

  return !/(one[- ]?time|otp|totp|token|verification|verify|code|\u9a8c\u8bc1\u7801|\u52a8\u6001|\u4ee4\u724c|\u4e00\u6b21)/iu.test(text);
}

function createKeyboardInteractiveHandler(sender, endpoint, sshConfig) {
  return async ({ name, instructions, prompts }) => {
    const normalizedPrompts = normalizeKeyboardPrompts(prompts);

    if (!normalizedPrompts.length) {
      return [];
    }

    const savedPassword = typeof sshConfig?.password === 'string' ? sshConfig.password : '';

    if (savedPassword && normalizedPrompts.every(isPasswordKeyboardPrompt)) {
      return normalizedPrompts.map(() => savedPassword);
    }

    const response = await requestRendererDecision(
      sender,
      keyboardInteractiveRequests,
      'connection:keyboard-interactive',
      'keyboard',
      {
        hostname: endpoint.hostname,
        port: endpoint.port,
        username: endpoint.username,
        name: readPromptText(name),
        instructions: readPromptText(instructions),
        prompts: normalizedPrompts,
      },
    );

    if (response?.cancel) {
      return [];
    }

    const responses = Array.isArray(response?.responses) ? response.responses : [];
    return normalizedPrompts.map((_prompt, index) => String(responses[index] ?? ''));
  };
}

function upsertKnownHostFromVerification(hostKeyInfo) {
  const hostname = String(hostKeyInfo.hostname || '').trim();
  const port = Number(hostKeyInfo.port) || 22;
  const keyType = String(hostKeyInfo.keyType || '').trim();
  const fingerprint = normalizeFingerprint(hostKeyInfo.fingerprint);

  if (!hostname || !fingerprint) {
    return null;
  }

  const vault = getVault();
  const currentKnownHosts = Array.isArray(vault.knownHosts) ? vault.knownHosts : [];
  const now = new Date().toISOString();
  const existingIndex = currentKnownHosts.findIndex((knownHost) => {
    if (hostKeyInfo.knownHostId && knownHost.id === hostKeyInfo.knownHostId) {
      return true;
    }

    if (!matchesHostAndPort(knownHost, hostname, port)) {
      return false;
    }

    const existingFingerprint = normalizeFingerprint(knownHost.fingerprint);
    return existingFingerprint === fingerprint || (keyType && knownHost.keyType === keyType);
  });
  const existing = existingIndex >= 0 ? currentKnownHosts[existingIndex] : null;
  const nextPublicKey = normalizeKnownHostPublicKey(hostKeyInfo.publicKey) ||
    normalizeKnownHostPublicKey(existing?.publicKey);
  const nextKnownHost = {
    id: existing?.id || crypto.randomUUID(),
    hostname,
    port,
    keyType,
    publicKey: nextPublicKey,
    fingerprint,
    discoveredAt: existing?.discoveredAt || now,
    lastSeen: now,
    convertedToHostId: existing?.convertedToHostId || '',
  };
  const nextKnownHosts = existingIndex >= 0
    ? currentKnownHosts.map((knownHost, index) => (index === existingIndex ? nextKnownHost : knownHost))
    : [nextKnownHost, ...currentKnownHosts];

  setVault({
    ...vault,
    knownHosts: nextKnownHosts,
  });
  notifyVaultChanged({ kind: 'vault' });
  return nextKnownHost;
}

function normalizeKnownHostPublicKey(value) {
  if (typeof value !== 'string') {
    return '';
  }

  const normalizedValue = value.replace(/\0/gu, ' ').trim();

  if (!normalizedValue || normalizedValue.length > 128 * 1024) {
    return '';
  }

  const match = normalizedValue.match(/(?:^|\s)([A-Za-z0-9@._+-]+)\s+([A-Za-z0-9+/]+={0,2})(?:\s|$)/u);

  if (!match) {
    return '';
  }

  return `${match[1]} ${match[2]}`;
}

function createHostVerifier(sender, endpoint) {
  return (rawKey, callback) => {
    const keyInfo = describeHostKey(rawKey);
    const knownHosts = getVault().knownHosts;
    const decision = classifyHostKey({
      knownHosts,
      hostname: endpoint.hostname,
      port: endpoint.port,
      keyType: keyInfo.keyType,
      fingerprint: keyInfo.fingerprint,
    });

    if (decision.status === 'trusted') {
      callback(true);
      return;
    }

    void requestRendererDecision(
      sender,
      hostKeyVerificationRequests,
      'connection:host-key-verification',
      'hostkey',
      {
        hostname: endpoint.hostname,
        port: endpoint.port,
        username: endpoint.username,
        status: decision.status,
        keyType: keyInfo.keyType,
        fingerprint: keyInfo.fingerprint,
        publicKey: keyInfo.publicKey,
        knownHostId: decision.knownHost?.id || '',
        knownFingerprint: decision.expectedFingerprint || '',
      },
    ).then((response) => {
      const accept = Boolean(response?.accept);

      if (accept && response?.addToKnownHosts) {
        try {
          upsertKnownHostFromVerification({
            ...endpoint,
            ...keyInfo,
            knownHostId: decision.knownHost?.id || '',
          });
        } catch (error) {
          console.warn(
            `[shelldesk] failed to save SSH known host ${endpoint.hostname}:${endpoint.port}:`,
            toErrorMessage(error),
          );
        }
      }

      callback(accept);
    }).catch((error) => {
      console.warn(
        `[shelldesk] host key verification request failed ${endpoint.hostname}:${endpoint.port}:`,
        toErrorMessage(error),
      );
      callback(false);
    });
  };
}

function createConnectionEndpoint(displayHost, sshConfig) {
  return {
    hostname: String(sshConfig?.host || displayHost?.address || '').trim(),
    port: Number(sshConfig?.port || displayHost?.port) || 22,
    username: String(sshConfig?.username || displayHost?.username || '').trim(),
  };
}

function applySshSecurityHandlers(sshConfig, endpoint, sender) {
  if (!sshConfig || !endpoint.hostname) {
    return;
  }

  sshConfig.tryKeyboard = true;
  sshConfig.hostVerifier = createHostVerifier(sender, endpoint);
  sshConfig.shellDeskKeyboardInteractiveHandler = createKeyboardInteractiveHandler(sender, endpoint, sshConfig);
}

function createProxyReuseDescriptor(proxyConfig) {
  if (!proxyConfig) {
    return null;
  }

  return {
    type: String(proxyConfig.type || ''),
    host: normalizeHostname(proxyConfig.host),
    port: Number(proxyConfig.port) || 0,
    command: proxyConfig.type === 'command' ? String(proxyConfig.command || '') : '',
    username: String(proxyConfig.username || ''),
  };
}

function createSshReuseDescriptor(sshConfig, displayHost, rawHost = {}) {
  const authMethod = String(displayHost?.authMethod || rawHost.authMethod || (
    sshConfig?.privateKey ? 'key' : sshConfig?.password ? 'password' : 'agent'
  ));

  return {
    host: normalizeHostname(sshConfig?.host || displayHost?.address),
    port: Number(sshConfig?.port || displayHost?.port) || 22,
    username: String(sshConfig?.username || displayHost?.username || ''),
    authMethod,
    hostId: String(rawHost.id || ''),
    keyId: authMethod === 'key' ? String(rawHost.keyId || '') : '',
    usesKeyPath: authMethod === 'key' && Boolean(rawHost.keyPath),
    usesPassword: authMethod === 'password' && Boolean(sshConfig?.password),
    usesAgent: authMethod === 'agent' || Boolean(sshConfig?.authHandler),
  };
}

function createConnectionReuseKey(rawHost, validated) {
  const target = createSshReuseDescriptor(validated.sshConfig, validated.displayHost, rawHost);
  const jump = validated.jumpSshConfig
    ? {
        target: createSshReuseDescriptor(validated.jumpSshConfig, validated.jumpHost, {}),
        proxy: createProxyReuseDescriptor(validated.jumpProxyConfig),
      }
    : null;

  return JSON.stringify({
    target,
    proxy: createProxyReuseDescriptor(validated.proxyConfig),
    jump,
  });
}

function getCertificateTrustOrigin(rawUrl) {
  try {
    const url = new URL(String(rawUrl || ''));

    if (url.protocol !== 'https:') {
      return null;
    }

    return url.origin.toLowerCase();
  } catch {
    return null;
  }
}

function getActiveConnectionByPartition(partition) {
  for (const activeConnection of activeConnections.values()) {
    if (activeConnection.partition === partition) {
      return activeConnection;
    }
  }

  return null;
}

function getActiveConnectionBySession(targetSession) {
  for (const activeConnection of activeConnections.values()) {
    if (activeConnection.browserSession === targetSession) {
      return activeConnection;
    }
  }

  return null;
}

function handleBrowserCertificateError(event, webContents, url, error, _certificate, callback) {
  const activeConnection = getActiveConnectionBySession(webContents.session);
  const trustOrigin = getCertificateTrustOrigin(url);

  if (!activeConnection || !trustOrigin || !activeConnection.browserCertificateTrust?.has(trustOrigin)) {
    callback(false);
    return;
  }

  event.preventDefault();
  console.info(`[shelldesk] trusted browser certificate for ${trustOrigin}: ${error}`);
  callback(true);
}

function registerConnectionHandlers(registerIpcHandler) {
  if (!isBrowserCertificateHandlerRegistered) {
    app.on('certificate-error', handleBrowserCertificateError);
    isBrowserCertificateHandlerRegistered = true;
  }

  registerIpcHandler('connection:keyboard-interactive-response', async (event, payload) => {
    const result = settlePendingRequest(keyboardInteractiveRequests, payload, {
      responses: Array.isArray(payload?.responses) ? payload.responses.map((response) => String(response ?? '')) : [],
      cancel: Boolean(payload?.cancel),
    }, event);

    if (!result.success) {
      throw new Error(result.error);
    }

    return true;
  });

  registerIpcHandler('connection:host-key-response', async (event, payload) => {
    const result = settlePendingRequest(hostKeyVerificationRequests, payload, {
      accept: Boolean(payload?.accept),
      addToKnownHosts: Boolean(payload?.addToKnownHosts),
    }, event);

    if (!result.success) {
      throw new Error(result.error);
    }

    return true;
  });

  ipcMain.handle('connection:connect', async (event, rawHost) => {
    let client;
    let jumpClient;
    let activeConnection;

    try {
      const validated = validateHostRequest(rawHost);
      const { displayHost, sshConfig, privilegeConfig, proxyConfig, jumpSshConfig, jumpProxyConfig, jumpHost } = validated;
      const reuseKey = createConnectionReuseKey(rawHost, validated);
      const reusableConnection = findReusableActiveConnection(reuseKey);

      if (reusableConnection) {
        focusConnectionWindow(reusableConnection);
        return {
          ok: true,
          reused: true,
          connection: toConnectionInfo(reusableConnection),
        };
      }

      applySshSecurityHandlers(sshConfig, createConnectionEndpoint(displayHost, sshConfig), event.sender);

      if (jumpSshConfig) {
        applySshSecurityHandlers(jumpSshConfig, createConnectionEndpoint(jumpHost, jumpSshConfig), event.sender);
      }

      const connectedClients = await connectSshClientWithJump(sshConfig, jumpSshConfig, jumpHost, proxyConfig, jumpProxyConfig);
      client = connectedClients.client;
      jumpClient = connectedClients.jumpClient;
      try {
        Object.assign(displayHost, await detectRemoteSystem(client));
      } catch (systemError) {
        console.info(`[shelldesk] remote system detection failed: ${toErrorMessage(systemError)}`);
      }
      const id = crypto.randomUUID();
      const partition = `shelldesk-${id}`;
      const remoteSession = session.fromPartition(partition);
      activeConnection = {
        id,
        client: null,
        jumpClient,
        sshConfig,
        privilegeConfig,
        proxyConfig,
        jumpSshConfig,
        jumpProxyConfig,
        jumpHost,
        socksServer: null,
        proxyPort: 0,
        reuseKey,
        partition,
        browserSession: remoteSession,
        browserCertificateTrust: new Set(),
        displayHost,
        connectedAt: new Date().toISOString(),
        terminalSessions: new Map(),
        clientOnline: false,
        reconnectPromise: null,
        lastDisconnectReason: '',
      };

      bindActiveConnectionClient(activeConnection, client);
      activeConnections.set(id, activeConnection);
      const { server, port } = await createSocksProxy(async () => {
        const connection = await ensureActiveConnectionClient(id);
        return connection.client;
      });
      activeConnection.socksServer = server;
      activeConnection.proxyPort = port;

      await remoteSession.setProxy({
        mode: 'fixed_servers',
        proxyRules: `socks5://127.0.0.1:${port}`,
        proxyBypassRules: '<-loopback>',
      });
      const loopbackProxy = await remoteSession.resolveProxy('http://127.0.0.1/');
      const publicProxy = await remoteSession.resolveProxy('http://example.com/');
      console.info(`[shelldesk] webview proxy ${partition}: 127.0.0.1 => ${loopbackProxy}; example.com => ${publicProxy}`);
      remoteSession.setPermissionRequestHandler((_webContents, _permission, callback) => callback(false));
      createConnectionWindow(activeConnection);

      return {
        ok: true,
        connection: toConnectionInfo(activeConnection),
      };
    } catch (error) {
      if (activeConnection) {
        await closeActiveConnection(activeConnection.id, '连接初始化失败。').catch(() => undefined);
      } else {
        client?.end();
        jumpClient?.end();
      }
      return { ok: false, error: toConnectionErrorMessage(error) };
    }
  });

  ipcMain.handle('connection:open-local', async () => {
    const reuseKey = 'local';
    const reusableConnection = findReusableActiveConnection(reuseKey);

    if (reusableConnection) {
      focusConnectionWindow(reusableConnection);
      return {
        ok: true,
        reused: true,
        connection: toConnectionInfo(reusableConnection),
      };
    }

    const id = crypto.randomUUID();
    const partition = `shelldesk-${id}`;
    const localSession = session.fromPartition(partition);
    const activeConnection = {
      id,
      kind: 'local',
      client: createLocalClient(),
      jumpClient: null,
      sshConfig: null,
      privilegeConfig: null,
      proxyConfig: null,
      jumpSshConfig: null,
      jumpProxyConfig: null,
      jumpHost: null,
      socksServer: null,
      proxyPort: 0,
      reuseKey,
      partition,
      browserSession: localSession,
      browserCertificateTrust: new Set(),
      displayHost: createLocalDisplayHost(),
      connectedAt: new Date().toISOString(),
      terminalSessions: new Map(),
      clientOnline: true,
      reconnectPromise: null,
      lastDisconnectReason: '',
    };

    await localSession.setProxy({ mode: 'direct' });
    localSession.setPermissionRequestHandler((_webContents, _permission, callback) => callback(false));
    activeConnections.set(id, activeConnection);
    createConnectionWindow(activeConnection);

    return {
      ok: true,
      connection: toConnectionInfo(activeConnection),
    };
  });

  registerIpcHandler('connection:disconnect', async (_event, connectionId) => {
    const activeConnection = getActiveConnection(connectionId);
    await closeActiveConnection(connectionId, activeConnection.kind === 'local' ? '已关闭本地模式。' : '已断开 SSH 连接。');
    return true;
  });

  registerIpcHandler('connection:get-info', async (_event, connectionId) => {
    return toConnectionInfo(getActiveConnection(connectionId));
  });

  registerIpcHandler('connection:get-ipc-capabilities', async () => ({
    terminalSessions: true,
    terminalBinary: true,
  }));

  registerIpcHandler('connection:trust-browser-certificate', async (event, partition, rawUrl) => {
    const activeConnection = getActiveConnectionByPartition(String(partition || ''));

    if (!activeConnection) {
      throw new Error('浏览器连接已断开，无法信任该证书。');
    }

    if (activeConnection.window?.webContents !== event.sender) {
      throw new Error('只能在当前连接窗口内信任该证书。');
    }

    const trustOrigin = getCertificateTrustOrigin(rawUrl);

    if (!trustOrigin) {
      throw new Error('只能为 HTTPS 地址添加临时证书例外。');
    }

    activeConnection.browserCertificateTrust ??= new Set();
    activeConnection.browserCertificateTrust.add(trustOrigin);
    return { origin: trustOrigin };
  });

}

module.exports = { registerConnectionHandlers };

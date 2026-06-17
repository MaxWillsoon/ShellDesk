const crypto = require('node:crypto');
const { utils: sshUtils } = require('ssh2');

function normalizeFingerprint(value) {
  if (typeof value !== 'string') {
    return '';
  }

  return value
    .trim()
    .replace(/^SHA256:/iu, '')
    .replace(/=+$/gu, '');
}

function normalizeHostname(value) {
  return String(value || '').trim().toLowerCase();
}

function parseKnownHostPattern(hostname) {
  const value = String(hostname || '').trim();

  if (!value) {
    return { hostname: '', port: undefined };
  }

  const firstPattern = value.split(',', 1)[0] || '';
  const bracketMatch = firstPattern.match(/^\[([^\]]+)\]:(\d+)$/u);

  if (bracketMatch) {
    return {
      hostname: normalizeHostname(bracketMatch[1]),
      port: Number.parseInt(bracketMatch[2], 10),
    };
  }

  return { hostname: normalizeHostname(firstPattern), port: undefined };
}

function getKnownHostPort(knownHost) {
  const parsed = parseKnownHostPattern(knownHost?.hostname);

  if (Number.isFinite(knownHost?.port)) {
    return Number(knownHost.port);
  }

  if (Number.isFinite(parsed.port)) {
    return Number(parsed.port);
  }

  return 22;
}

function matchesHostAndPort(knownHost, hostname, port = 22) {
  const parsed = parseKnownHostPattern(knownHost?.hostname);

  if (!parsed.hostname || parsed.hostname.startsWith('|1|')) {
    return false;
  }

  return parsed.hostname === normalizeHostname(hostname) && getKnownHostPort(knownHost) === (Number(port) || 22);
}

function fingerprintFromPublicKey(publicKey) {
  if (typeof publicKey !== 'string') {
    return '';
  }

  const trimmed = publicKey.trim();

  if (!trimmed) {
    return '';
  }

  if (/^SHA256:/iu.test(trimmed)) {
    return normalizeFingerprint(trimmed);
  }

  const parts = trimmed.split(/\s+/u);

  if (parts.length >= 2 && /^(?:ssh-|ecdsa-|sk-)/iu.test(parts[0])) {
    try {
      return crypto.createHash('sha256')
        .update(Buffer.from(parts[1], 'base64'))
        .digest('base64')
        .replace(/=+$/gu, '');
    } catch {
      return '';
    }
  }

  return normalizeFingerprint(trimmed);
}

function getKnownHostFingerprint(knownHost) {
  return normalizeFingerprint(knownHost?.fingerprint) || fingerprintFromPublicKey(knownHost?.publicKey);
}

function describeRawPublicKeyBlob(rawKey) {
  const key = Buffer.isBuffer(rawKey) ? rawKey : Buffer.from(rawKey || '');

  if (key.length < 8) {
    return null;
  }

  const typeLength = key.readUInt32BE(0);

  if (typeLength <= 0 || typeLength > 128 || 4 + typeLength > key.length) {
    return null;
  }

  const keyType = key.subarray(4, 4 + typeLength).toString('ascii');

  if (!/^[A-Za-z0-9@._+-]+$/u.test(keyType)) {
    return null;
  }

  return {
    keyType,
    publicKey: `${keyType} ${key.toString('base64')}`,
  };
}

function isOpenSshPublicKeyText(value) {
  if (typeof value !== 'string') {
    return false;
  }

  const trimmed = value.trim();

  if (!trimmed || trimmed.length > 128 * 1024 || /[\0\r\n]/u.test(trimmed)) {
    return false;
  }

  const parts = trimmed.split(/\s+/u);
  return parts.length >= 2 &&
    /^[A-Za-z0-9@._+-]+$/u.test(parts[0]) &&
    /^[A-Za-z0-9+/]+={0,2}$/u.test(parts[1]);
}

function describeHostKey(rawKey) {
  const key = Buffer.isBuffer(rawKey) ? rawKey : Buffer.from(rawKey || '');
  const fingerprint = crypto.createHash('sha256')
    .update(key)
    .digest('base64')
    .replace(/=+$/gu, '');
  let keyType = 'unknown';
  let publicKey = '';

  const rawPublicKey = describeRawPublicKeyBlob(key);

  if (rawPublicKey) {
    keyType = rawPublicKey.keyType;
    publicKey = rawPublicKey.publicKey;
  }

  try {
    const parsed = sshUtils.parseKey(key);
    const parsedKey = Array.isArray(parsed) ? parsed[0] : parsed;

    if (parsedKey && !(parsedKey instanceof Error)) {
      keyType = parsedKey.type || keyType;
      const publicSsh = parsedKey.getPublicSSH?.();

      if (publicSsh) {
        const publicSshText = publicSsh.toString('utf8').trim();

        if (isOpenSshPublicKeyText(publicSshText)) {
          publicKey = publicSshText;
        }
      }
    }
  } catch {
    // The raw SSH key blob still gives us a reliable SHA256 fingerprint.
  }

  return { keyType, fingerprint, publicKey };
}

function classifyHostKey({ knownHosts = [], hostname, port = 22, keyType, fingerprint }) {
  const normalizedFingerprint = normalizeFingerprint(fingerprint);
  const candidates = Array.isArray(knownHosts)
    ? knownHosts.filter((knownHost) => matchesHostAndPort(knownHost, hostname, port))
    : [];

  if (!candidates.length) {
    return { status: 'unknown' };
  }

  const comparableCandidates = candidates
    .map((knownHost) => ({
      knownHost,
      fingerprint: getKnownHostFingerprint(knownHost),
    }))
    .filter((entry) => entry.fingerprint);

  const match = comparableCandidates.find((entry) => entry.fingerprint === normalizedFingerprint);

  if (match) {
    return { status: 'trusted', knownHost: match.knownHost };
  }

  const normalizedKeyType = String(keyType || '').trim();

  if (normalizedKeyType && normalizedKeyType !== 'unknown') {
    const sameTypeMismatch = comparableCandidates.find((entry) => entry.knownHost.keyType === normalizedKeyType);

    if (sameTypeMismatch) {
      return {
        status: 'changed',
        knownHost: sameTypeMismatch.knownHost,
        expectedFingerprint: sameTypeMismatch.fingerprint,
      };
    }
  }

  return { status: 'unknown' };
}

module.exports = {
  classifyHostKey,
  describeHostKey,
  getKnownHostFingerprint,
  matchesHostAndPort,
  normalizeFingerprint,
  normalizeHostname,
};

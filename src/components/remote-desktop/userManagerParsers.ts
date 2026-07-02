import type { RemoteGroupRecord, RemoteUserDetail, RemoteUserManagerSnapshot, RemoteUserRecord, UserPasswordStatus } from './userManagerTypes';

const snapshotMarkers = {
  passwd: '__SHELLDESK_PASSWD__',
  group: '__SHELLDESK_GROUP__',
  passwdStatus: '__SHELLDESK_PASSWD_STATUS__',
  sudoers: '__SHELLDESK_SUDOERS__',
} as const;

const detailMarkers = {
  groups: '__SHELLDESK_DETAIL_GROUPS__',
  sshKeys: '__SHELLDESK_DETAIL_SSH_KEYS__',
  lastlog: '__SHELLDESK_DETAIL_LASTLOG__',
  chage: '__SHELLDESK_DETAIL_CHAGE__',
} as const;

function splitSections<T extends Record<string, string>>(raw: string, markers: T) {
  const sections = new Map<keyof T, string[]>();
  let current: keyof T | null = null;
  const markerEntries = Object.entries(markers) as Array<[keyof T, string]>;

  for (const line of raw.replace(/\r/g, '').split('\n')) {
    const marker = markerEntries.find(([, value]) => value === line.trim());
    if (marker) {
      current = marker[0];
      sections.set(current, []);
      continue;
    }

    if (current) {
      sections.get(current)?.push(line);
    }
  }

  return sections;
}

function parsePasswordStatusToken(value: string): UserPasswordStatus {
  if (value === 'P' || value === 'PS') return 'active';
  if (value === 'L' || value === 'LK') return 'locked';
  if (value === 'NP') return 'no-password';
  return 'unknown';
}

function parsePasswordStatus(lines: string[]) {
  const map = new Map<string, UserPasswordStatus>();

  for (const line of lines) {
    const [username, status] = line.trim().split(/\s+/);
    if (username && status) {
      map.set(username, parsePasswordStatusToken(status));
    }
  }

  return map;
}

function parseGroups(lines: string[]): RemoteGroupRecord[] {
  return lines
    .map((line) => {
      const [name, , gidValue, membersValue = ''] = line.split(':');
      const gid = Number.parseInt(gidValue, 10);
      if (!name || !Number.isFinite(gid)) return null;
      return {
        name,
        gid,
        members: membersValue.split(',').map((item) => item.trim()).filter(Boolean).sort((a, b) => a.localeCompare(b)),
      };
    })
    .filter((item): item is RemoteGroupRecord => Boolean(item))
    .sort((a, b) => a.gid - b.gid || a.name.localeCompare(b.name));
}

function getPrimaryGroupName(groups: RemoteGroupRecord[], gid: number) {
  return groups.find((group) => group.gid === gid)?.name ?? String(gid);
}

function getSupplementaryGroups(groups: RemoteGroupRecord[], username: string) {
  return groups
    .filter((group) => group.members.includes(username))
    .map((group) => group.name)
    .sort((a, b) => a.localeCompare(b));
}

function parseUsers(lines: string[], groups: RemoteGroupRecord[], passwordStatus: Map<string, UserPasswordStatus>): RemoteUserRecord[] {
  return lines
    .map((line) => {
      const [username, , uidValue, gidValue, , home = '', shell = ''] = line.split(':');
      const uid = Number.parseInt(uidValue, 10);
      const gid = Number.parseInt(gidValue, 10);
      if (!username || !Number.isFinite(uid) || !Number.isFinite(gid)) return null;

      return {
        username,
        uid,
        gid,
        primaryGroup: getPrimaryGroupName(groups, gid),
        home,
        shell,
        passwordStatus: passwordStatus.get(username) ?? 'unknown',
        supplementaryGroups: getSupplementaryGroups(groups, username),
        isSystemUser: uid < 1000 && username !== 'root',
      };
    })
    .filter((item): item is RemoteUserRecord => Boolean(item))
    .sort((a, b) => a.uid - b.uid || a.username.localeCompare(b.username));
}

export function parseUserManagerSnapshot(raw: string): RemoteUserManagerSnapshot {
  const sections = splitSections(raw, snapshotMarkers);
  const groups = parseGroups(sections.get('group') ?? []);
  const passwordStatus = parsePasswordStatus(sections.get('passwdStatus') ?? []);
  const users = parseUsers(sections.get('passwd') ?? [], groups, passwordStatus);
  const sudoersLines = (sections.get('sudoers') ?? []).map((line) => line.trim()).filter(Boolean);

  return { users, groups, sudoersLines };
}

export function parseUserDetail(username: string, raw: string): RemoteUserDetail {
  const sections = splitSections(raw, detailMarkers);
  const groupLine = (sections.get('groups') ?? []).find((line) => line.trim()) ?? '';
  const keyLine = (sections.get('sshKeys') ?? []).find((line) => line.trim()) ?? '';
  const lastlogLine = (sections.get('lastlog') ?? []).find((line) => line.trim()) ?? '';
  const passwordAging = (sections.get('chage') ?? []).join('\n').trim();
  const sshKeyCount = /^\d+$/.test(keyLine.trim()) ? Number.parseInt(keyLine.trim(), 10) : null;

  return {
    username,
    supplementaryGroups: groupLine.split(/\s+/).map((item) => item.trim()).filter(Boolean),
    sshKeyCount,
    sshKeysReadable: sshKeyCount !== null,
    lastLogin: lastlogLine || '',
    passwordAging,
  };
}

import { shellQuote } from './settingsShared';
import type { AddGroupDraft, AddUserDraft, EditUserDraft, RemoteUserRecord } from './userManagerTypes';

export const USER_MANAGER_SNAPSHOT_COMMAND = `
printf '__SHELLDESK_PASSWD__\\n'
(getent passwd 2>/dev/null || cat /etc/passwd 2>/dev/null || true)
printf '__SHELLDESK_GROUP__\\n'
(getent group 2>/dev/null || cat /etc/group 2>/dev/null || true)
printf '__SHELLDESK_PASSWD_STATUS__\\n'
if command -v passwd >/dev/null 2>&1; then passwd -Sa 2>/dev/null || true; fi
printf '__SHELLDESK_SUDOERS__\\n'
if [ -r /etc/sudoers ]; then awk 'NF && $1 !~ /^#/ { print FILENAME ":" $0 }' /etc/sudoers 2>/dev/null; fi
if [ -d /etc/sudoers.d ]; then
  for file in /etc/sudoers.d/*; do
    [ -f "$file" ] && [ -r "$file" ] && awk 'NF && $1 !~ /^#/ { print FILENAME ":" $0 }' "$file" 2>/dev/null
  done
fi
`;

const accountNamePattern = /^[a-z_][a-z0-9_.-]{0,30}\$?$/i;
const absolutePathPattern = /^\/[^\0]*$/;

export function isSafeAccountName(value: string) {
  return accountNamePattern.test(value.trim());
}

export function isSafeNumericId(value: string) {
  return /^\d{1,10}$/.test(value.trim());
}

export function isSafeAbsolutePath(value: string) {
  return absolutePathPattern.test(value.trim());
}

function splitCsv(value: string) {
  return value
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function parseGroupInput(value: string) {
  return splitCsv(value);
}

export function validateAddUserDraft(draft: AddUserDraft) {
  const username = draft.username.trim();
  if (!isSafeAccountName(username)) return 'username';
  if (draft.uid.trim() && !isSafeNumericId(draft.uid)) return 'uid';
  if (draft.primaryGroup.trim() && !isSafeAccountName(draft.primaryGroup)) return 'primaryGroup';
  if (draft.home.trim() && !isSafeAbsolutePath(draft.home)) return 'home';
  if (draft.shell.trim() && !isSafeAbsolutePath(draft.shell)) return 'shell';
  if (parseGroupInput(draft.supplementaryGroups).some((group) => !isSafeAccountName(group))) return 'supplementaryGroups';
  return '';
}

export function validateEditUserDraft(draft: EditUserDraft) {
  if (!isSafeAccountName(draft.username)) return 'username';
  if (draft.uid.trim() && !isSafeNumericId(draft.uid)) return 'uid';
  if (draft.primaryGroup.trim() && !isSafeAccountName(draft.primaryGroup)) return 'primaryGroup';
  if (draft.home.trim() && !isSafeAbsolutePath(draft.home)) return 'home';
  if (draft.shell.trim() && !isSafeAbsolutePath(draft.shell)) return 'shell';
  return '';
}

export function validateAddGroupDraft(draft: AddGroupDraft) {
  if (!isSafeAccountName(draft.name)) return 'group';
  if (draft.gid.trim() && !isSafeNumericId(draft.gid)) return 'gid';
  return '';
}

export function createEditDraftFromUser(user: RemoteUserRecord): EditUserDraft {
  return {
    username: user.username,
    uid: String(user.uid),
    primaryGroup: user.primaryGroup,
    home: user.home,
    shell: user.shell,
    moveHome: false,
  };
}

export function buildAddUserCommand(draft: AddUserDraft) {
  const args = ['useradd', draft.createHome ? '-m' : '-M'];
  const uid = draft.uid.trim();
  const primaryGroup = draft.primaryGroup.trim();
  const home = draft.home.trim();
  const shell = draft.shell.trim();
  const supplementaryGroups = parseGroupInput(draft.supplementaryGroups);

  if (uid) args.push('-u', shellQuote(uid));
  if (primaryGroup) args.push('-g', shellQuote(primaryGroup));
  if (supplementaryGroups.length) args.push('-G', shellQuote(supplementaryGroups.join(',')));
  if (home) args.push('-d', shellQuote(home));
  if (shell) args.push('-s', shellQuote(shell));
  args.push(shellQuote(draft.username.trim()));
  return args.join(' ');
}

export function buildEditUserCommand(draft: EditUserDraft) {
  const args = ['usermod'];
  const uid = draft.uid.trim();
  const primaryGroup = draft.primaryGroup.trim();
  const home = draft.home.trim();
  const shell = draft.shell.trim();

  if (uid) args.push('-u', shellQuote(uid));
  if (primaryGroup) args.push('-g', shellQuote(primaryGroup));
  if (home) {
    args.push('-d', shellQuote(home));
    if (draft.moveHome) args.push('-m');
  }
  if (shell) args.push('-s', shellQuote(shell));
  args.push(shellQuote(draft.username.trim()));
  return args.join(' ');
}

export function buildDeleteUserCommand(username: string, deleteHome: boolean) {
  return ['userdel', deleteHome ? '-r' : '', shellQuote(username.trim())].filter(Boolean).join(' ');
}

export function buildUserPasswordCommand(username: string, action: 'lock' | 'unlock' | 'expire') {
  if (action === 'expire') {
    return `chage -d 0 ${shellQuote(username.trim())}`;
  }
  return `passwd ${action === 'lock' ? '-l' : '-u'} ${shellQuote(username.trim())}`;
}

export function buildAddGroupCommand(draft: AddGroupDraft) {
  const args = ['groupadd'];
  if (draft.gid.trim()) args.push('-g', shellQuote(draft.gid.trim()));
  args.push(shellQuote(draft.name.trim()));
  return args.join(' ');
}

export function buildDeleteGroupCommand(groupName: string) {
  return `groupdel ${shellQuote(groupName.trim())}`;
}

export function buildGroupMemberCommand(username: string, groupName: string, action: 'add' | 'remove') {
  return `gpasswd ${action === 'add' ? '-a' : '-d'} ${shellQuote(username.trim())} ${shellQuote(groupName.trim())}`;
}

export function buildUserDetailCommand(username: string, home: string) {
  const quotedUser = shellQuote(username.trim());
  const keyPath = shellQuote(`${home.replace(/\/$/, '')}/.ssh/authorized_keys`);
  return `
printf '__SHELLDESK_DETAIL_GROUPS__\\n'
id -nG ${quotedUser} 2>/dev/null || true
printf '__SHELLDESK_DETAIL_SSH_KEYS__\\n'
if [ -r ${keyPath} ]; then awk 'NF && $1 !~ /^#/' ${keyPath} 2>/dev/null | wc -l; else printf 'UNREADABLE\\n'; fi
printf '__SHELLDESK_DETAIL_LASTLOG__\\n'
if command -v lastlog >/dev/null 2>&1; then lastlog -u ${quotedUser} 2>/dev/null | tail -n +2; fi
printf '__SHELLDESK_DETAIL_CHAGE__\\n'
if command -v chage >/dev/null 2>&1; then chage -l ${quotedUser} 2>&1 || true; fi
`;
}

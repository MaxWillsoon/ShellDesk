export type UserPasswordStatus = 'active' | 'locked' | 'no-password' | 'unknown';

export interface RemoteUserRecord {
  username: string;
  uid: number;
  gid: number;
  primaryGroup: string;
  home: string;
  shell: string;
  passwordStatus: UserPasswordStatus;
  supplementaryGroups: string[];
  isSystemUser: boolean;
}

export interface RemoteGroupRecord {
  name: string;
  gid: number;
  members: string[];
}

export interface RemoteUserManagerSnapshot {
  users: RemoteUserRecord[];
  groups: RemoteGroupRecord[];
  sudoersLines: string[];
}

export interface RemoteUserDetail {
  username: string;
  supplementaryGroups: string[];
  sshKeyCount: number | null;
  sshKeysReadable: boolean;
  lastLogin: string;
  passwordAging: string;
}

export interface AddUserDraft {
  username: string;
  uid: string;
  primaryGroup: string;
  home: string;
  shell: string;
  supplementaryGroups: string;
  createHome: boolean;
}

export interface EditUserDraft {
  username: string;
  uid: string;
  primaryGroup: string;
  home: string;
  shell: string;
  moveHome: boolean;
}

export interface AddGroupDraft {
  name: string;
  gid: string;
}

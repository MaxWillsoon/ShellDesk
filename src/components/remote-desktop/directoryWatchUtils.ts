import type { RemoteFileEntry } from './fileExplorerTypes';

export const FILE_EXPLORER_DIRECTORY_WATCH_INTERVAL_MS = 4000;
export const CODE_EDITOR_DIRECTORY_WATCH_INTERVAL_MS = 5000;
export const CODE_EDITOR_MAX_WATCHED_DIRECTORIES = 40;
export const CODE_EDITOR_FILE_WATCH_INTERVAL_MS = 3000;
export const CODE_EDITOR_MAX_WATCHED_FILES = 12;

export function createDirectoryEntriesSignature(entries: RemoteFileEntry[]) {
  return entries
    .map((entry) => [
      entry.name,
      entry.type,
      entry.targetType ?? '',
      entry.targetPath ?? '',
      entry.size,
      entry.modifiedAt,
      entry.longname,
    ].join('\x1f'))
    .sort()
    .join('\x1e');
}

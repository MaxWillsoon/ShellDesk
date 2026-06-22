import type { RemoteSystemType } from './types';

export interface NotepadTab {
  id: string;
  filePath?: string;
  title: string;
  content: string;
  originalContent: string;
  dirty: boolean;
  readOnly: boolean;
  revisionHint?: string;
  language: string;
  languageManuallySet: boolean;
  isLoading: boolean;
  isSaving: boolean;
  error: string;
}

export interface RemoteNotepadProps {
  connectionId: string;
  settings: ShellDeskAppSettings;
  initialFilePath?: string;
  initialContent?: string;
  initialTitle?: string;
  openFileRequest?: NotepadOpenFileRequest;
  systemType?: RemoteSystemType;
}

export interface NotepadOpenFileRequest {
  id: string;
  filePath: string;
}

export interface SaveOptions {
  closeAfterSave?: boolean;
  force?: boolean;
}

export interface NotepadConflictDialog {
  tabId: string;
  title: string;
  filePath: string;
  remoteContent?: string;
  remoteRevisionHint?: string;
  readError?: string;
  closeAfterSave: boolean;
}

export interface NotepadDiffDialog {
  tabId: string;
  title: string;
  beforeLabel: string;
  beforeContent: string;
  afterLabel: string;
  afterContent: string;
}

export type NotepadSudoOperation = 'read' | 'save';

export interface NotepadSudoPrompt {
  operation: NotepadSudoOperation;
  filePath: string;
  error: string;
  password: string;
}

export interface DiffPreviewLine {
  kind: 'context' | 'added' | 'removed' | 'meta';
  text: string;
}

export interface DiffPreview {
  lines: DiffPreviewLine[];
  truncated: boolean;
}

export type NotepadAiMessageRole = 'user' | 'assistant' | 'tool';

export type NotepadAiAction =
  | {
      type: 'replace_content' | 'append_content' | 'insert_at_cursor' | 'replace_selection';
      content: string;
      summary?: string;
    }
  | {
      type: 'run_command';
      command: string;
      reason?: string;
    };

export interface AiEditOperation {
  messageId: string;
  action: Exclude<NotepadAiAction, { type: 'run_command' }>;
  selection: EditorSelectionSnapshot;
}

export interface AiEditState {
  isOpen: boolean;
}

export interface NotepadAiMessage {
  id: string;
  role: NotepadAiMessageRole;
  content: string;
  createdAt: string;
  action?: NotepadAiAction;
  actionApplied?: boolean;
}

export interface EditorSelectionSnapshot {
  start: number;
  end: number;
  text: string;
}

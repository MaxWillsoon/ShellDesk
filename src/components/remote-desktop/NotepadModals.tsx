import { type RefObject } from 'react';
import { createPortal } from 'react-dom';

import { t, type AppLanguage } from '../../i18n';
import { NotepadDiffPreview } from './notepadDiff';
import type {
  DiffPreview,
  NotepadConflictDialog,
  NotepadDiffDialog,
  NotepadSudoPrompt,
  SaveOptions,
} from './notepadTypes';

interface NotepadModalsProps {
  language: AppLanguage;
  pendingCloseTab: { id: string; title: string } | null;
  diffDialog: NotepadDiffDialog | null;
  diffPreview: DiffPreview | null;
  conflictDialog: NotepadConflictDialog | null;
  conflictPreview: DiffPreview | null;
  sudoPrompt: NotepadSudoPrompt | null;
  sudoPasswordInputRef: RefObject<HTMLInputElement | null>;
  onCancelPendingClose: () => void;
  onDiscardClose: (tabId: string) => void;
  onSaveTab: (tabId: string, options?: SaveOptions) => void;
  onCloseDiff: () => void;
  onCloseConflict: () => void;
  onReloadConflict: (dialog: NotepadConflictDialog) => void;
  onOpenSavePicker: (tabId: string, title: string, closeAfterSave?: boolean) => void;
  onForceSave: (tabId: string, filePath: string, options: SaveOptions) => void;
  onResolveSudoPrompt: (password: string | null) => void;
  onChangeSudoPrompt: (sudoPrompt: NotepadSudoPrompt) => void;
}

export default function NotepadModals({
  language,
  pendingCloseTab,
  diffDialog,
  diffPreview,
  conflictDialog,
  conflictPreview,
  sudoPrompt,
  sudoPasswordInputRef,
  onCancelPendingClose,
  onDiscardClose,
  onSaveTab,
  onCloseDiff,
  onCloseConflict,
  onReloadConflict,
  onOpenSavePicker,
  onForceSave,
  onResolveSudoPrompt,
  onChangeSudoPrompt,
}: NotepadModalsProps) {
  return (
    <>
      {pendingCloseTab ? createPortal(
        <div className="notepad-modal-overlay" role="presentation" onClick={onCancelPendingClose}>
          <div
            className="notepad-modal"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="notepad-close-title"
            onClick={(event) => event.stopPropagation()}
          >
            <div id="notepad-close-title" className="notepad-modal-title">{t('notepad.modal.unsavedTitle', language)}</div>
            <div className="notepad-modal-message">{t('notepad.modal.unsavedMessage', language, { title: pendingCloseTab.title })}</div>
            <div className="notepad-modal-actions">
              <button type="button" className="notepad-modal-btn" onClick={() => onDiscardClose(pendingCloseTab.id)}>{t('notepad.modal.discardClose', language)}</button>
              <button type="button" className="notepad-modal-btn" onClick={onCancelPendingClose}>{t('common.cancel', language)}</button>
              <button type="button" className="notepad-modal-btn primary" onClick={() => onSaveTab(pendingCloseTab.id, { closeAfterSave: true })}>{t('notepad.modal.saveClose', language)}</button>
            </div>
          </div>
        </div>,
        document.body,
      ) : null}

      {diffDialog && diffPreview ? createPortal(
        <div className="notepad-modal-overlay" role="presentation" onClick={onCloseDiff}>
          <div
            className="notepad-modal notepad-diff-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="notepad-diff-title"
            onClick={(event) => event.stopPropagation()}
          >
            <div id="notepad-diff-title" className="notepad-modal-title">{t('notepad.modal.diffTitle', language, { title: diffDialog.title })}</div>
            <div className="notepad-diff-legend">
              <span>{diffDialog.beforeLabel}</span>
              <span>{diffDialog.afterLabel}</span>
            </div>
            <NotepadDiffPreview preview={diffPreview} language={language} />
            <div className="notepad-modal-actions">
              <button type="button" className="notepad-modal-btn" onClick={onCloseDiff}>{t('common.close', language)}</button>
              <button type="button" className="notepad-modal-btn primary" onClick={() => onSaveTab(diffDialog.tabId)}>{t('common.save', language)}</button>
            </div>
          </div>
        </div>,
        document.body,
      ) : null}

      {conflictDialog ? createPortal(
        <div className="notepad-modal-overlay" role="presentation" onClick={onCloseConflict}>
          <div
            className="notepad-modal notepad-conflict-modal"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="notepad-conflict-title"
            onClick={(event) => event.stopPropagation()}
          >
            <div id="notepad-conflict-title" className="notepad-modal-title">{t('notepad.modal.remoteChangedTitle', language)}</div>
            <div className="notepad-modal-message">
              {conflictDialog.readError
                ? t('notepad.modal.conflictReadFailed', language, { path: conflictDialog.filePath, error: conflictDialog.readError })
                : t('notepad.modal.conflictMessage', language, { title: conflictDialog.title })}
            </div>
            {conflictPreview ? <NotepadDiffPreview preview={conflictPreview} language={language} /> : null}
            <div className="notepad-modal-actions">
              <button type="button" className="notepad-modal-btn" onClick={onCloseConflict}>{t('common.cancel', language)}</button>
              <button
                type="button"
                className="notepad-modal-btn"
                disabled={conflictDialog.remoteContent === undefined}
                onClick={() => onReloadConflict(conflictDialog)}
              >
                {t('notepad.modal.reload', language)}
              </button>
              <button type="button" className="notepad-modal-btn" onClick={() => {
                onOpenSavePicker(conflictDialog.tabId, t('notepad.picker.saveConflictAs', language), conflictDialog.closeAfterSave);
                onCloseConflict();
              }}>{t('notepad.toolbar.saveAs', language)}</button>
              <button type="button" className="notepad-modal-btn danger" onClick={() => onForceSave(conflictDialog.tabId, conflictDialog.filePath, {
                force: true,
                closeAfterSave: conflictDialog.closeAfterSave,
              })}>{t('notepad.modal.overwriteRemote', language)}</button>
            </div>
          </div>
        </div>,
        document.body,
      ) : null}

      {sudoPrompt ? createPortal(
        <div className="notepad-modal-overlay" role="presentation" onClick={() => onResolveSudoPrompt(null)}>
          <form
            className="notepad-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="notepad-sudo-title"
            onClick={(event) => event.stopPropagation()}
            onSubmit={(event) => {
              event.preventDefault();
              onResolveSudoPrompt(sudoPrompt.password);
            }}
          >
            <div id="notepad-sudo-title" className="notepad-modal-title">
              {t(sudoPrompt.operation === 'read' ? 'notepad.sudo.title.read' : 'notepad.sudo.title.save', language)}
            </div>
            <div className="notepad-modal-message">
              {t('notepad.sudo.message', language, {
                operation: t(sudoPrompt.operation === 'read' ? 'notepad.sudo.operation.read' : 'notepad.sudo.operation.save', language),
                path: sudoPrompt.filePath,
              })}
            </div>
            {sudoPrompt.error ? <div className="notepad-modal-message">{t('notepad.sudo.lastError', language, { error: sudoPrompt.error })}</div> : null}
            <label className="notepad-modal-field">
              <span>{t('notepad.sudo.password', language)}</span>
              <input
                ref={sudoPasswordInputRef}
                className="notepad-modal-input"
                type="password"
                value={sudoPrompt.password}
                placeholder={t('notepad.sudo.passwordPlaceholder', language)}
                onChange={(event) => onChangeSudoPrompt({ ...sudoPrompt, password: event.target.value })}
              />
            </label>
            <div className="notepad-modal-actions">
              <button type="button" className="notepad-modal-btn" onClick={() => onResolveSudoPrompt(null)}>{t('common.cancel', language)}</button>
              <button type="submit" className="notepad-modal-btn primary" disabled={!sudoPrompt.password}>{t('notepad.sudo.submit', language)}</button>
            </div>
          </form>
        </div>,
        document.body,
      ) : null}
    </>
  );
}

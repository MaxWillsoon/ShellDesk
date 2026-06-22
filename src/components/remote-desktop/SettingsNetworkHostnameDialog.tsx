import { createPortal } from 'react-dom';

import { t } from '../../i18n';
import type { SettingsNetworkHostnameDialogProps } from './settingsTypes';
import { SettingsCommandPreview, shellQuote } from './settingsShared';

export default function SettingsNetworkHostnameDialog({
  hostnameDraft,
  language,
  onClose,
  onSave,
  setHostnameDraft,
}: SettingsNetworkHostnameDialogProps) {
  return createPortal(
    <div className="notepad-modal-overlay" role="presentation" onClick={onClose}>
      <div
        className="notepad-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="hostname-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div id="hostname-dialog-title" className="notepad-modal-title">{t('remoteSettings.network.hostnameDialogTitle', language)}</div>
        <input
          className="notepad-modal-input"
          value={hostnameDraft}
          onChange={(event) => setHostnameDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              onSave();
            }
            if (event.key === 'Escape') {
              event.preventDefault();
              onClose();
            }
          }}
          autoFocus
          placeholder={t('remoteSettings.network.hostnamePlaceholder', language)}
        />
        {hostnameDraft.trim() ? (
          <SettingsCommandPreview
            label={t('remoteSettings.common.preview', language)}
            content={[
              `hostnamectl set-hostname ${shellQuote(hostnameDraft.trim())}`,
              `hostname ${shellQuote(hostnameDraft.trim())}`,
            ].join('\n')}
          />
        ) : null}
        <div className="notepad-modal-actions">
          <button type="button" className="notepad-modal-btn" onClick={onClose}>{t('remoteSettings.common.cancel', language)}</button>
          <button type="button" className="notepad-modal-btn primary" onClick={onSave}>{t('remoteSettings.common.save', language)}</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

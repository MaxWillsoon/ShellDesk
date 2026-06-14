import { Copy, KeyRound, MoreHorizontal, Pencil, Trash2 } from 'lucide-react';

import { t, useCurrentAppLanguage } from '../i18n';

interface SshKey {
  id: string;
  name: string;
  source: 'imported' | 'generated';
  algorithm: string;
  fingerprint: string;
  publicKey: string;
  passphrase: string;
  createdAt: string;
  updatedAt: string;
}

interface KeysPageProps {
  keySearchQuery: string;
  filteredKeys: SshKey[];
  sshKeys: SshKey[];
  onSearchChange: (value: string) => void;
  onImportPrivateKey: () => void;
  onCreateKey: () => void;
  onEditKey: (key: SshKey) => void;
  onDeleteKey: (key: SshKey) => void;
  onCopyPublicKey: (key: SshKey) => void;
}

function closeKeyCardMenu(target: HTMLElement) {
  const menu = target.closest('details');

  if (menu instanceof HTMLDetailsElement) {
    menu.open = false;
  }
}

function KeysPage({
  keySearchQuery,
  filteredKeys,
  sshKeys,
  onSearchChange,
  onImportPrivateKey,
  onCreateKey,
  onEditKey,
  onDeleteKey,
  onCopyPublicKey,
}: KeysPageProps) {
  const language = useCurrentAppLanguage();
  const getKeySourceLabel = (source: SshKey['source']) => (
    source === 'generated'
      ? (language === 'zh-CN' ? '生成' : 'Generated')
      : (language === 'zh-CN' ? '导入' : 'Imported')
  );

  return (
    <>
      <div className="command-bar no-drag key-command-bar">
        <label className="global-search">
          <span>{t('keys.search.label', language)}</span>
          <input
            type="search"
            placeholder={t('keys.search.placeholder', language)}
            value={keySearchQuery}
            onChange={(event) => onSearchChange(event.target.value)}
          />
        </label>

        <button type="button" className="command-button key-import-button" onClick={onImportPrivateKey}>{t('keys.import', language)}</button>
        <button type="button" className="primary-action key-create-button" onClick={onCreateKey}>{t('keys.createRsa', language)}</button>
      </div>

      <section className="vault-content hosts-content network-assets-content key-assets-content">
        <section className="vault-section host-section hosts-list-panel network-list-panel key-list-panel">
          <div className="section-heading host-list-heading">
            <div className="host-list-title">
              <h2>{t('keys.list', language)} <b>{filteredKeys.length}</b></h2>
            </div>
            <span className="host-list-controls">
              {t('keys.count', language, { count: filteredKeys.length })}
              {keySearchQuery.trim() ? (
                <button
                  type="button"
                  className="host-refresh-button network-clear-filter"
                  onClick={() => onSearchChange('')}
                  aria-label={language === 'zh-CN' ? '清除搜索' : 'Clear search'}
                  title={language === 'zh-CN' ? '清除搜索' : 'Clear search'}
                >
                  <span aria-hidden="true">×</span>
                </button>
              ) : null}
            </span>
          </div>

          <div className="host-list-scroll key-list-scroll">
        {filteredKeys.length ? (
          <div className="host-grid grid key-grid key-card-grid">
            {filteredKeys.map((key) => (
              <article key={key.id} className="host-card key-card">
                <button type="button" className="host-card-main key-card-main" onClick={() => onEditKey(key)}>
                  <span className="host-avatar key-card-icon" aria-hidden="true">
                    <KeyRound />
                  </span>
                  <span className="host-summary key-card-summary">
                    <strong>{key.name}</strong>
                    <small>{key.fingerprint || (key.publicKey ? t('keys.publicKey.loaded', language) : t('keys.publicKey.missing', language))}</small>
                    <span className="host-card-tags key-card-tags">
                      <em>{key.algorithm || 'SSH'}</em>
                      <em>{getKeySourceLabel(key.source)}</em>
                    </span>
                  </span>
                </button>
                <span className="host-card-actions key-card-actions">
                  <details className="host-card-menu key-card-menu" onClick={(event) => event.stopPropagation()}>
                    <summary aria-label={language === 'zh-CN' ? '密钥操作' : 'Key actions'}>
                      <MoreHorizontal aria-hidden="true" />
                    </summary>
                    <div className="host-card-menu-panel">
                      {key.publicKey ? (
                        <button
                          type="button"
                          onClick={(event) => {
                            closeKeyCardMenu(event.currentTarget);
                            onCopyPublicKey(key);
                          }}
                        >
                          <Copy aria-hidden="true" />
                          {t('keys.copyPublicKey', language)}
                        </button>
                      ) : null}
                      <button
                        type="button"
                        onClick={(event) => {
                          closeKeyCardMenu(event.currentTarget);
                          onEditKey(key);
                        }}
                      >
                        <Pencil aria-hidden="true" />
                        {t('keys.edit', language)}
                      </button>
                      <button
                        type="button"
                        className="danger-text"
                        onClick={(event) => {
                          closeKeyCardMenu(event.currentTarget);
                          onDeleteKey(key);
                        }}
                      >
                        <Trash2 aria-hidden="true" />
                        {t('keys.delete', language)}
                      </button>
                    </div>
                  </details>
                </span>
              </article>
            ))}
          </div>
        ) : (
          <div className="empty-state">
            <span>KEYS</span>
            <h3>{sshKeys.length ? t('keys.empty.noMatches.title', language) : t('keys.empty.noKeys.title', language)}</h3>
            <p>{sshKeys.length ? t('keys.empty.noMatches.description', language) : t('keys.empty.noKeys.description', language)}</p>
          </div>
        )}
          </div>
        </section>
      </section>
    </>
  );
}

export default KeysPage;

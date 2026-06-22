import { useCallback, useEffect, useState } from 'react';

import { getErrorMessage } from './desktopUtils';
import DismissibleAlert from './DismissibleAlert';
import { t, useCurrentAppLanguage } from '../../i18n';
import type { DnsConfig, IfaceEditState, NetworkInterface, RemoteSettingsSectionState, SettingsConfirmDialogConfig } from './settingsTypes';
import { areDnsConfigsEqual, buildResolvConfContent, createDnsConfigPreview, netmaskToPrefix, parseIpAddr, parseResolvConf, prefixToNetmask } from './settingsParsers';
import { isSafeHostname, isSafeNameserver, SettingsConfirmDialog, shellQuote, useRemoteSettingsCommand, withLinuxPrivilege } from './settingsShared';
import SettingsNetworkHostnameDialog from './SettingsNetworkHostnameDialog';

export default function SettingsNetworkPanel() {
  const language = useCurrentAppLanguage();
  const runCommand = useRemoteSettingsCommand();
  const [ifaces, setIfaces] = useState<NetworkInterface[]>([]);
  const EMPTY_DNS_CONFIG: DnsConfig = { servers: [], search: '', raw: '' };
  const [dnsState, setDnsState] = useState<RemoteSettingsSectionState<DnsConfig>>({
    loaded: false,
    loading: false,
    current: EMPTY_DNS_CONFIG,
    draft: EMPTY_DNS_CONFIG,
  });
  const [hostname, setHostname] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');
  const [editingIface, setEditingIface] = useState<string | null>(null);
  const [editForm, setEditForm] = useState<IfaceEditState>({ method: 'dhcp', address: '', netmask: '', gateway: '' });
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [newDns, setNewDns] = useState('');
  const [isHostnameDialogOpen, setIsHostnameDialogOpen] = useState(false);
  const [hostnameDraft, setHostnameDraft] = useState('');
  const [confirmDialog, setConfirmDialog] = useState<SettingsConfirmDialogConfig | null>(null);
  const refresh = useCallback(async () => {
    setLoading(true);
    setDnsState((currentState) => ({ ...currentState, loading: true, error: undefined }));
    setError('');
    try {
      const [ifResult, dnsResult, hostResult] = await Promise.all([
        runCommand('ip addr show 2>/dev/null || ifconfig -a 2>/dev/null'),
        runCommand('cat /etc/resolv.conf 2>/dev/null'),
        runCommand('hostname -f 2>/dev/null || hostname'),
      ]);
      setIfaces(parseIpAddr(ifResult.stdout || ''));
      const dnsConfig = parseResolvConf(dnsResult.stdout || '');
      setDnsState({
        loaded: true,
        loading: false,
        current: dnsConfig,
        draft: dnsConfig,
      });
      setHostname((hostResult.stdout || '').trim());
    } catch (err) {
      const message = getErrorMessage(err);
      setError(message);
      setDnsState((currentState) => ({ ...currentState, loading: false, error: message }));
    } finally {
      setLoading(false);
      setDnsState((currentState) => ({ ...currentState, loading: false }));
    }
  }, [runCommand]);
  useEffect(() => { void refresh(); }, [refresh]);
  const applyIfacePowerState = async (ifaceName: string, bringUp: boolean) => {
    setActionLoading(ifaceName);
    setError('');
    setSuccess('');
    try {
      const result = await runCommand(withLinuxPrivilege(`ip link set ${shellQuote(ifaceName)} ${bringUp ? 'up' : 'down'} 2>&1`));
      if (result.code !== 0) throw new Error(result.stderr || t('remoteSettings.common.operationFailedRoot', language));
      setSuccess(t('remoteSettings.network.ifacePowerSuccess', language, {
        name: ifaceName,
        state: t(bringUp ? 'remoteSettings.network.enabled' : 'remoteSettings.network.disabled', language),
      }));
      await refresh();
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setActionLoading(null);
    }
  };
  const requestToggleIface = (ifaceName: string, bringUp: boolean) => {
    setConfirmDialog({
      title: t(bringUp ? 'remoteSettings.network.enableIfaceTitle' : 'remoteSettings.network.disableIfaceTitle', language),
      message: bringUp
        ? t('remoteSettings.network.enableIfaceMessage', language, { name: ifaceName })
        : t('remoteSettings.network.disableIfaceMessage', language, { name: ifaceName }),
      detail: t('remoteSettings.network.ifacePowerDetail', language),
      preview: `ip link set ${shellQuote(ifaceName)} ${bringUp ? 'up' : 'down'}`,
      confirmLabel: t(bringUp ? 'remoteSettings.network.enableIfaceConfirm' : 'remoteSettings.network.disableIfaceConfirm', language),
      tone: bringUp ? 'warning' : 'danger',
      onConfirm: () => applyIfacePowerState(ifaceName, bringUp),
    });
  };
  const startEditIface = (iface: NetworkInterface) => {
    setEditingIface(iface.name);
    setSuccess('');
    setError('');
    const ipv4 = iface.addresses.find((a) => a.family === 'inet');
    setEditForm({
      method: 'static',
      address: ipv4?.addr ?? '',
      netmask: ipv4 ? prefixToNetmask(ipv4.prefixLen) : '255.255.255.0',
      gateway: '',
    });
  };
  const buildIfaceConfigPlan = (ifaceName: string, form: IfaceEditState) => {
    const ifaceArg = shellQuote(ifaceName);
    if (form.method === 'dhcp') {
      return {
        command: `dhclient -r ${ifaceArg} 2>/dev/null; ip -4 addr flush dev ${ifaceArg} scope global 2>&1 && dhclient ${ifaceArg} 2>&1 || echo ${shellQuote(t('remoteSettings.network.dhclientUnavailable', language))}`,
        preview: [`dhclient -r ${ifaceArg}`, `ip -4 addr flush dev ${ifaceArg} scope global`, `dhclient ${ifaceArg}`].join('\n'),
      };
    }
    if (!form.address.trim()) {
      throw new Error(t('remoteSettings.network.ipRequired', language));
    }
    if (!isSafeNameserver(form.address.trim())) {
      throw new Error(t('remoteSettings.network.ipInvalid', language));
    }
    if (form.gateway.trim() && !isSafeNameserver(form.gateway.trim())) {
      throw new Error(t('remoteSettings.network.gatewayInvalid', language));
    }
    const prefix = form.netmask.trim() ? netmaskToPrefix(form.netmask.trim()) : 24;
    if (prefix === null) {
      throw new Error(t('remoteSettings.network.netmaskInvalid', language));
    }
    const cidr = `${form.address.trim()}/${prefix}`;
    let command = `ip -4 addr flush dev ${ifaceArg} scope global 2>&1 && ip addr add ${shellQuote(cidr)} dev ${ifaceArg} 2>&1 && ip link set ${ifaceArg} up 2>&1`;
    const previewLines = [
      `ip -4 addr flush dev ${ifaceArg} scope global`,
      `ip addr add ${shellQuote(cidr)} dev ${ifaceArg}`,
      `ip link set ${ifaceArg} up`,
    ];
    if (form.gateway.trim()) {
      command += ` && ip route replace default via ${shellQuote(form.gateway.trim())} dev ${ifaceArg} 2>&1`;
      previewLines.push(`ip route replace default via ${shellQuote(form.gateway.trim())} dev ${ifaceArg}`);
    }
    return {
      command,
      preview: previewLines.join('\n'),
    };
  };
  const applyIfaceConfig = async (ifaceName: string, command: string) => {
    setActionLoading(ifaceName);
    setError('');
    setSuccess('');
    try {
      const result = await runCommand(withLinuxPrivilege(command));
      if (result.code !== 0 && !result.stdout.includes('dhclient')) {
        throw new Error(result.stderr || result.stdout || t('remoteSettings.common.configFailedRoot', language));
      }
      setSuccess(t('remoteSettings.network.ifaceConfigSuccess', language, { name: ifaceName }));
      setEditingIface(null);
      await refresh();
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setActionLoading(null);
    }
  };
  const requestApplyIfaceConfig = () => {
    if (!editingIface) return;
    try {
      const plan = buildIfaceConfigPlan(editingIface, editForm);
      setConfirmDialog({
        title: t('remoteSettings.network.applyIfaceTitle', language),
        message: t('remoteSettings.network.applyIfaceMessage', language, { name: editingIface }),
        detail: t('remoteSettings.network.applyIfaceDetail', language),
        preview: plan.preview,
        confirmLabel: t('remoteSettings.common.applyConfig', language),
        tone: 'danger',
        onConfirm: () => applyIfaceConfig(editingIface, plan.command),
      });
    } catch (err) {
      setError(getErrorMessage(err));
    }
  };
  const openHostnameDialog = () => {
    setHostnameDraft(hostname);
    setError('');
    setIsHostnameDialogOpen(true);
  };
  const setHostnameCmd = async () => {
    const name = hostnameDraft.trim();
    if (!name) {
      setError(t('remoteSettings.network.hostnameRequired', language));
      return;
    }
    if (!isSafeHostname(name)) {
      setError(t('remoteSettings.network.hostnameInvalid', language));
      return;
    }
    setIsHostnameDialogOpen(false);
    setActionLoading('hostname');
    setError('');
    setSuccess('');
    try {
      const quotedName = shellQuote(name);
      const result = await runCommand(withLinuxPrivilege(`hostnamectl set-hostname ${quotedName} 2>&1 || hostname ${quotedName} 2>&1`));
      if (result.code !== 0) throw new Error(result.stderr || t('remoteSettings.network.hostnameFailed', language));
      setSuccess(t('remoteSettings.network.hostnameSuccess', language, { name }));
      setHostname(name);
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setActionLoading(null);
    }
  };
  const currentDnsConfig = dnsState.current ?? EMPTY_DNS_CONFIG;
  const dnsDraftConfig = dnsState.draft ?? currentDnsConfig;
  const isDnsDirty = !areDnsConfigsEqual(currentDnsConfig, dnsDraftConfig);
  const addDnsServer = () => {
    const server = newDns.trim();
    if (!server) return;
    if (!isSafeNameserver(server)) {
      setError(t('remoteSettings.network.dnsInvalid', language));
      return;
    }
    if (dnsDraftConfig.servers.includes(server)) {
      setSuccess(t('remoteSettings.network.dnsAlreadyInDraft', language, { server }));
      setNewDns('');
      return;
    }
    setError('');
    setSuccess('');
    setDnsState((currentState) => ({
      ...currentState,
      draft: {
        ...(currentState.draft ?? EMPTY_DNS_CONFIG),
        servers: [...(currentState.draft?.servers ?? []), server],
      },
      success: t('remoteSettings.network.dnsAddedDraft', language, { server }),
    }));
    setNewDns('');
  };
  const removeDnsServer = (server: string) => {
    setError('');
    setSuccess('');
    setDnsState((currentState) => ({
      ...currentState,
      draft: {
        ...(currentState.draft ?? EMPTY_DNS_CONFIG),
        servers: (currentState.draft?.servers ?? []).filter((item) => item !== server),
      },
      success: t('remoteSettings.network.dnsRemovedDraft', language, { server }),
    }));
  };
  const applyDnsDraft = async (nextContent: string, draft: DnsConfig) => {
    setActionLoading('dns');
    setError('');
    setSuccess('');
    try {
      const result = await runCommand(withLinuxPrivilege(`cp /etc/resolv.conf /etc/resolv.conf.bak.$(date +%s) 2>/dev/null; printf '%s' ${shellQuote(nextContent)} > /etc/resolv.conf`));
      if (result.code !== 0) {
        throw new Error(result.stderr || result.stdout || t('remoteSettings.network.dnsWriteFailed', language));
      }
      setDnsState((currentState) => ({
        ...currentState,
        current: { ...draft, raw: nextContent },
        draft: { ...draft, raw: nextContent },
        success: t('remoteSettings.network.dnsApplied', language),
      }));
      setSuccess(t('remoteSettings.network.dnsApplied', language));
      await refresh();
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setActionLoading(null);
    }
  };
  const requestApplyDnsDraft = () => {
    if (!isDnsDirty) return;
    const nextContent = buildResolvConfContent(currentDnsConfig.raw, dnsDraftConfig);
    setConfirmDialog({
      title: t('remoteSettings.network.applyDnsTitle', language),
      message: t('remoteSettings.network.applyDnsMessage', language),
      detail: t('remoteSettings.network.applyDnsDetail', language),
      preview: createDnsConfigPreview(currentDnsConfig, dnsDraftConfig, language),
      confirmLabel: t('remoteSettings.network.applyDnsConfirm', language),
      tone: 'warning',
      onConfirm: () => applyDnsDraft(nextContent, dnsDraftConfig),
    });
  };
  return (
    <div className="settings-panel-content">
      <div className="settings-panel-header">
        <div>
          <h3>{t('remoteSettings.network.title', language)}</h3>
          <p>{t('remoteSettings.network.description', language)}</p>
        </div>
        <button type="button" className="settings-action-btn" onClick={refresh} disabled={loading}>
          {loading ? t('remoteSettings.common.loading', language) : t('remoteSettings.common.refresh', language)}
        </button>
      </div>
      {error ? (
        <DismissibleAlert className="error-banner" source="RemoteSettings" onDismiss={() => setError('')} role="alert">
          {error}
        </DismissibleAlert>
      ) : null}
      {success ? (
        <DismissibleAlert className="settings-success-banner" onDismiss={() => setSuccess('')}>
          {success}
        </DismissibleAlert>
      ) : null}
      <div className="settings-warning-banner">
        {t('remoteSettings.network.warning', language)}
      </div>
      <div className="settings-info-card">
        <div className="settings-info-row">
          <span className="settings-info-label">{t('remoteSettings.network.hostname', language)}</span>
          <strong className="settings-info-value">{hostname || '...'}</strong>
        </div>
        <button type="button" className="settings-action-btn" onClick={openHostnameDialog} disabled={actionLoading === 'hostname'}>
          {actionLoading === 'hostname' ? '...' : t('remoteSettings.network.change', language)}
        </button>
      </div>
      <div className="settings-section">
        <h4>{t('remoteSettings.network.interfaces', language, { count: String(ifaces.length) })}</h4>
        <div className="net-iface-grid">
          {ifaces.map((iface) => {
            const ipv4 = iface.addresses.filter((a) => a.family === 'inet');
            const ipv6 = iface.addresses.filter((a) => a.family === 'inet6');
            const isEditing = editingIface === iface.name;
            const isBusy = actionLoading === iface.name;
            return (
              <article key={iface.name} className={`net-iface-card ${iface.state === 'UP' ? 'up' : 'down'}`}>
                <div className="net-iface-header">
                  <div className="net-iface-title">
                    <span className={`net-iface-state-dot ${iface.state === 'UP' ? 'up' : 'down'}`} />
                    <strong>{iface.name}</strong>
                    <span className={`net-iface-state-tag ${iface.state === 'UP' ? 'up' : 'down'}`}>{iface.state}</span>
                  </div>
                  <div className="net-iface-actions">
                    {iface.state === 'UP' ? (
                      <button type="button" className="settings-action-btn danger" onClick={() => requestToggleIface(iface.name, false)} disabled={isBusy}>
                        {isBusy ? '...' : t('remoteSettings.network.disable', language)}
                      </button>
                    ) : (
                      <button type="button" className="settings-action-btn primary" onClick={() => requestToggleIface(iface.name, true)} disabled={isBusy}>
                        {isBusy ? '...' : t('remoteSettings.network.enable', language)}
                      </button>
                    )}
                    <button type="button" className="settings-action-btn" onClick={() => startEditIface(iface)} disabled={isBusy}>
                      {t('remoteSettings.common.edit', language)}
                    </button>
                  </div>
                </div>
                <div className="net-iface-info">
                  {iface.mac ? <span className="net-iface-meta"><em>MAC</em>{iface.mac}</span> : null}
                  <span className="net-iface-meta"><em>MTU</em>{iface.mtu}</span>
                  {ipv4.map((a) => (
                    <span key={a.addr} className="net-iface-meta ipv4">
                      <em>IPv4</em>{a.addr} / {a.prefixLen}
                      <small>({prefixToNetmask(a.prefixLen)})</small>
                    </span>
                  ))}
                  {ipv6.map((a) => (
                    <span key={a.addr} className="net-iface-meta ipv6">
                      <em>IPv6</em>{a.addr} / {a.prefixLen}
                    </span>
                  ))}
                  {ipv4.length === 0 && ipv6.length === 0 ? (
                    <span className="net-iface-meta no-addr">{t('remoteSettings.network.noAddress', language)}</span>
                  ) : null}
                </div>
                {isEditing ? (
                  <div className="net-iface-edit">
                    <div className="net-edit-row">
                      <label>
                        <span>{t('remoteSettings.network.configMethod', language)}</span>
                        <select
                          value={editForm.method}
                          onChange={(e) => setEditForm({ ...editForm, method: e.target.value as 'static' | 'dhcp' })}
                          className="settings-select"
                        >
                          <option value="static">{t('remoteSettings.network.staticIp', language)}</option>
                          <option value="dhcp">{t('remoteSettings.network.dhcpAuto', language)}</option>
                        </select>
                      </label>
                    </div>
                    {editForm.method === 'static' ? (
                      <>
                        <div className="net-edit-row">
                          <label>
                            <span>{t('remoteSettings.network.ipAddress', language)}</span>
                            <input type="text" className="settings-input" placeholder="192.168.1.100" value={editForm.address} onChange={(e) => setEditForm({ ...editForm, address: e.target.value })} />
                          </label>
                          <label>
                            <span>{t('remoteSettings.network.netmask', language)}</span>
                            <input type="text" className="settings-input" placeholder="255.255.255.0" value={editForm.netmask} onChange={(e) => setEditForm({ ...editForm, netmask: e.target.value })} />
                          </label>
                        </div>
                        <div className="net-edit-row">
                          <label>
                            <span>{t('remoteSettings.network.gatewayOptional', language)}</span>
                            <input type="text" className="settings-input" placeholder="192.168.1.1" value={editForm.gateway} onChange={(e) => setEditForm({ ...editForm, gateway: e.target.value })} />
                          </label>
                        </div>
                      </>
                    ) : null}
                    <div className="net-edit-footer">
                      <button type="button" className="settings-action-btn" onClick={() => setEditingIface(null)}>{t('remoteSettings.common.cancel', language)}</button>
                      <button type="button" className="settings-action-btn primary" onClick={requestApplyIfaceConfig} disabled={isBusy}>
                        {isBusy ? t('remoteSettings.common.applyingConfig', language) : t('remoteSettings.common.applyConfig', language)}
                      </button>
                    </div>
                  </div>
                ) : null}
              </article>
            );
          })}
          {ifaces.length === 0 ? <p className="settings-hint">{loading ? t('remoteSettings.network.interfacesLoading', language) : t('remoteSettings.network.noInterfaces', language)}</p> : null}
        </div>
      </div>
      <div className="settings-section">
        <h4>{t('remoteSettings.network.dnsServers', language)}</h4>
        <div className="dns-server-list">
          {dnsDraftConfig.servers.map((server) => (
            <div key={server} className="dns-server-item">
              <span className="dns-server-addr">{server}</span>
              <button type="button" className="settings-action-btn danger" onClick={() => removeDnsServer(server)}>{t('remoteSettings.common.remove', language)}</button>
            </div>
          ))}
          {dnsDraftConfig.servers.length === 0 ? <p className="settings-hint">{dnsState.loading ? t('remoteSettings.network.dnsLoading', language) : t('remoteSettings.network.dnsEmpty', language)}</p> : null}
        </div>
        <div className="settings-inline-form">
          <input type="text" className="settings-input" placeholder={t('remoteSettings.network.addDnsPlaceholder', language)} value={newDns} onChange={(e) => setNewDns(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') void addDnsServer(); }} />
          <button type="button" className="settings-action-btn primary" onClick={addDnsServer}>{t('remoteSettings.network.addDraft', language)}</button>
        </div>
        <label className="settings-field">
          <span>{t('remoteSettings.network.searchDomain', language)}</span>
          <input
            type="text"
            className="settings-input"
            placeholder="example.com corp.local"
            value={dnsDraftConfig.search}
            onChange={(event) => {
              const search = event.target.value;
              setDnsState((currentState) => ({
                ...currentState,
                draft: {
                  ...(currentState.draft ?? EMPTY_DNS_CONFIG),
                  search,
                },
              }));
            }}
          />
        </label>
        {isDnsDirty ? (
          <div className="settings-draft-footer">
            <span>{t('remoteSettings.network.dnsDraftPending', language)}</span>
            <div className="settings-header-actions">
              <button type="button" className="settings-action-btn" onClick={() => setDnsState((currentState) => ({ ...currentState, draft: currentState.current }))}>
                {t('remoteSettings.network.rollbackDraft', language)}
              </button>
              <button type="button" className="settings-action-btn primary" onClick={requestApplyDnsDraft} disabled={actionLoading === 'dns'}>
                {actionLoading === 'dns' ? t('remoteSettings.common.applyingConfig', language) : t('remoteSettings.network.previewApply', language)}
              </button>
            </div>
          </div>
        ) : null}
      </div>
      {isHostnameDialogOpen ? (
        <SettingsNetworkHostnameDialog
          hostnameDraft={hostnameDraft}
          language={language}
          onClose={() => setIsHostnameDialogOpen(false)}
          onSave={() => void setHostnameCmd()}
          setHostnameDraft={setHostnameDraft}
        />
      ) : null}
      {confirmDialog ? <SettingsConfirmDialog config={confirmDialog} onClose={() => setConfirmDialog(null)} /> : null}
    </div>
  );
}

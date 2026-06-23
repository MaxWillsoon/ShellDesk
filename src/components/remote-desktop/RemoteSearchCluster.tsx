import { useCallback, useEffect, useMemo, useState } from 'react';
import { json } from '@codemirror/lang-json';
import { indentWithTab } from '@codemirror/commands';
import type { Extension } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import CodeMirror from '@uiw/react-codemirror';
import DismissibleAlert from './DismissibleAlert';

import { getErrorMessage, getShellDeskLocale } from './desktopUtils';
import { loadRemoteConnectionProfile, readProfileBoolean, readProfileString, saveRemoteConnectionProfile } from './remoteConnectionProfiles';
import {
  createSearchClusterTunnelRequest,
  normalizeIndices,
  normalizeShards,
  parseJsonResponse,
  type SearchClusterHealth,
  type SearchClusterIndex,
  type SearchClusterShard,
} from './searchClusterUtils';
import type { RemoteSystemType } from './types';
import { tCurrent } from '../../i18n';

interface RemoteSearchClusterProps {
  connectionId: string;
  hostId: string;
  systemType?: RemoteSystemType;
}

type SearchTab = 'overview' | 'shards' | 'query' | 'raw';

function stringifyJson(value: unknown) {
  const text = JSON.stringify(value, null, 2);
  return text === undefined ? '' : text;
}

function getHealthTone(status?: string) {
  if (status === 'green') return 'green';
  if (status === 'yellow') return 'yellow';
  if (status === 'red') return 'red';
  return 'unknown';
}

function getHttpTunnelApi() {
  const api = window.guiSSH?.connections;

  if (!api) {
    throw new Error(tCurrent('auto.remoteSearchCluster.g77vf3'));
  }

  return api;
}

function RemoteSearchCluster({ connectionId, hostId }: RemoteSearchClusterProps) {
  const [url, setUrl] = useState('http://127.0.0.1:9200');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [timeoutSeconds, setTimeoutSeconds] = useState('10');
  const [ignoreSslCertificate, setIgnoreSslCertificate] = useState(false);
  const [health, setHealth] = useState<SearchClusterHealth | null>(null);
  const [indices, setIndices] = useState<SearchClusterIndex[]>([]);
  const [shards, setShards] = useState<SearchClusterShard[]>([]);
  const [selectedIndexName, setSelectedIndexName] = useState('');
  const [indexSearch, setIndexSearch] = useState('');
  const [activeTab, setActiveTab] = useState<SearchTab>('overview');
  const [queryIndex, setQueryIndex] = useState('');
  const [queryBody, setQueryBody] = useState('{\n  "query": {\n    "match_all": {}\n  },\n  "size": 10\n}');
  const [queryResponse, setQueryResponse] = useState('');
  const [rawResponse, setRawResponse] = useState('');
  const [loading, setLoading] = useState(false);
  const [queryRunning, setQueryRunning] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [lastRefreshedAt, setLastRefreshedAt] = useState('');
  const queryEditorExtensions = useMemo<Extension[]>(() => [
    keymap.of([indentWithTab]),
    json(),
    EditorView.theme({
      '&': {
        height: '100%',
        minHeight: '0',
        border: '1px solid var(--search-border)',
        borderRadius: '8px',
        backgroundColor: 'rgba(5, 10, 16, 0.38)',
        color: 'var(--search-text)',
        fontSize: '12px',
      },
      '.cm-scroller': {
        backgroundColor: 'rgba(5, 10, 16, 0.38)',
        fontFamily: 'var(--font-mono, "Cascadia Mono", Consolas, monospace)',
        lineHeight: '20px',
      },
      '.cm-content': {
        padding: '10px 0',
        caretColor: 'var(--search-text)',
      },
      '.cm-line': {
        padding: '0 10px',
      },
      '.cm-gutters': {
        borderRight: '1px solid var(--search-border)',
        backgroundColor: 'rgba(255, 255, 255, 0.04)',
        color: 'var(--search-muted)',
      },
      '.cm-activeLine': {
        backgroundColor: 'rgba(103, 183, 255, 0.08)',
      },
      '.cm-activeLineGutter': {
        backgroundColor: 'rgba(103, 183, 255, 0.12)',
      },
      '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': {
        backgroundColor: 'rgba(103, 183, 255, 0.26)',
      },
      '&.cm-focused': {
        outline: 'none',
        borderColor: 'rgba(103, 183, 255, 0.45)',
        boxShadow: '0 0 0 3px rgba(103, 183, 255, 0.12)',
      },
    }),
  ], []);

  const config = useMemo(() => ({
    url,
    username,
    password,
    timeoutSeconds: Number.parseInt(timeoutSeconds, 10) || 10,
    ignoreSslCertificate,
  }), [ignoreSslCertificate, password, timeoutSeconds, url, username]);

  useEffect(() => {
    let disposed = false;

    void loadRemoteConnectionProfile(hostId, 'search-cluster').then((profile) => {
      if (disposed || !profile) return;

      setUrl(readProfileString(profile, 'url', 'http://127.0.0.1:9200'));
      setUsername(readProfileString(profile, 'username', ''));
      setPassword(readProfileString(profile, 'password', ''));
      setTimeoutSeconds(readProfileString(profile, 'timeoutSeconds', '10'));
      setIgnoreSslCertificate(readProfileBoolean(profile, 'ignoreSslCertificate', false));
    });

    return () => {
      disposed = true;
    };
  }, [hostId]);

  const filteredIndices = useMemo(() => {
    const keyword = indexSearch.trim().toLowerCase();
    return keyword ? indices.filter((index) => index.index.toLowerCase().includes(keyword)) : indices;
  }, [indexSearch, indices]);

  const selectedIndex = useMemo(() => {
    return indices.find((index) => index.index === selectedIndexName) ?? indices[0] ?? null;
  }, [indices, selectedIndexName]);

  const selectedIndexShards = useMemo(() => {
    if (!selectedIndex) return shards;
    return shards.filter((shard) => shard.index === selectedIndex.index);
  }, [selectedIndex, shards]);

  const executeJsonRequest = useCallback(async <T,>(path: string, label: string): Promise<T> => {
    const response = await getHttpTunnelApi().httpTunnelGet({ ...createSearchClusterTunnelRequest(config, path), connectionId });
    setRawResponse(stringifyJson(response));
    return parseJsonResponse<T>(response, label);
  }, [config, connectionId]);

  const refreshCluster = useCallback(async () => {
    setLoading(true);
    setError('');
    setNotice('');

    try {
      const [nextHealth, indexRows, shardRows] = await Promise.all([
        executeJsonRequest<SearchClusterHealth>('/_cluster/health', 'Cluster health'),
        executeJsonRequest<Array<Record<string, unknown>>>('/_cat/indices?format=json&bytes=b', 'Indices'),
        executeJsonRequest<Array<Record<string, unknown>>>('/_cat/shards?format=json&bytes=b', 'Shards'),
      ]);
      const nextIndices = normalizeIndices(indexRows);
      const nextShards = normalizeShards(shardRows);

      setHealth(nextHealth);
      setIndices(nextIndices);
      setShards(nextShards);
      setSelectedIndexName((current) => current && nextIndices.some((index) => index.index === current) ? current : nextIndices[0]?.index ?? '');
      setQueryIndex((current) => current || nextIndices[0]?.index || '');
      setLastRefreshedAt(new Date().toLocaleTimeString(getShellDeskLocale()));
      setNotice(tCurrent('auto.remoteSearchCluster.5mg5ut', { value0: nextIndices.length, value1: nextShards.length }));
      void saveRemoteConnectionProfile(hostId, 'search-cluster', {
        url,
        username,
        password,
        timeoutSeconds: String(config.timeoutSeconds),
        ignoreSslCertificate,
      }).catch(() => undefined);
    } catch (error) {
      setError(getErrorMessage(error));
    } finally {
      setLoading(false);
    }
  }, [config.timeoutSeconds, executeJsonRequest, hostId, ignoreSslCertificate, password, url, username]);

  const runSearch = async () => {
    const indexName = queryIndex.trim() || selectedIndex?.index || '';

    if (!indexName) {
      setError(tCurrent('auto.remoteSearchCluster.13slyer'));
      return;
    }

    try {
      JSON.parse(queryBody);
    } catch (error) {
      setError(tCurrent('auto.remoteSearchCluster.1hg3jw2', { value0: getErrorMessage(error) }));
      return;
    }

    setQueryRunning(true);
    setError('');
    setNotice('');

    try {
      const startedAt = performance.now();
      const response = parseJsonResponse<unknown>(
        await getHttpTunnelApi().httpTunnelPost({
          ...createSearchClusterTunnelRequest(config, `/${encodeURIComponent(indexName)}/_search`, JSON.parse(queryBody)),
          connectionId,
        }),
        '_search',
      );
      const durationMs = Math.round(performance.now() - startedAt);

      setQueryResponse(stringifyJson(response));
      setRawResponse(stringifyJson(response));
      setActiveTab('query');
      setNotice(tCurrent('auto.remoteSearchCluster.16hu75t', { value0: durationMs }));
    } catch (error) {
      setError(getErrorMessage(error));
    } finally {
      setQueryRunning(false);
    }
  };

  const formatQueryBody = () => {
    try {
      setQueryBody(stringifyJson(JSON.parse(queryBody)));
      setNotice(tCurrent('auto.remoteSearchCluster.ed12q0'));
      setError('');
    } catch (error) {
      setError(tCurrent('auto.remoteSearchCluster.usdhbr', { value0: getErrorMessage(error) }));
    }
  };

  const copyDiagnostics = async () => {
    await navigator.clipboard.writeText(stringifyJson({
      health,
      selectedIndex,
      shards: selectedIndexShards,
      queryResponse: queryResponse ? JSON.parse(queryResponse) : undefined,
    }));
    setNotice(tCurrent('auto.remoteSearchCluster.1ywwjjz'));
  };

  return (
    <section className="search-cluster">
      <header className="search-cluster-toolbar">
        <div className={`search-health ${getHealthTone(health?.status)}`}>
          <span>Cluster</span>
          <strong>{health?.cluster_name ?? 'Elasticsearch / OpenSearch'}</strong>
          <em>{health?.status ?? 'not connected'}</em>
        </div>
        <label>
          <span>URL</span>
          <input value={url} onChange={(event) => setUrl(event.target.value)} />
        </label>
        <label>
          <span>User</span>
          <input value={username} onChange={(event) => setUsername(event.target.value)} placeholder={tCurrent('auto.remoteSearchCluster.zflkxh')} />
        </label>
        <label>
          <span>Password</span>
          <input type="password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder={tCurrent('auto.remoteSearchCluster.zflkxh2')} />
        </label>
        <label className="timeout">
          <span>Timeout</span>
          <input value={timeoutSeconds} onChange={(event) => setTimeoutSeconds(event.target.value)} inputMode="numeric" />
        </label>
        <label className="search-tls-option" title={tCurrent('auto.remoteSearchCluster.1qsbxe5')}>
          <input type="checkbox" checked={ignoreSslCertificate} onChange={(event) => setIgnoreSslCertificate(event.target.checked)} />
          <span>{tCurrent('auto.remoteSearchCluster.1g5852k')}</span>
        </label>
        <button type="button" className="primary" onClick={refreshCluster} disabled={loading}>
          {loading ? tCurrent('auto.remoteSearchCluster.1taxqz1') : tCurrent('auto.remoteSearchCluster.13cmkv8')}
        </button>
      </header>

      {error ? <DismissibleAlert className="search-alert danger" onDismiss={() => setError('')} role="alert">{error}</DismissibleAlert> : null}
      {notice ? <DismissibleAlert className="search-alert info" onDismiss={() => setNotice('')}>{notice}</DismissibleAlert> : null}

      <div className="search-cluster-layout">
        <aside className="search-index-panel">
          <div className="search-panel-head">
            <div>
              <strong>{tCurrent('auto.remoteSearchCluster.1lig4k0')}</strong>
              <span>{indices.length} {tCurrent('auto.remoteSearchCluster.1ubrrxs')}{lastRefreshedAt || tCurrent('auto.remoteSearchCluster.1t0b1fu')}</span>
            </div>
            <button type="button" onClick={copyDiagnostics} disabled={!health}>{tCurrent('auto.remoteSearchCluster.4zl8tz')}</button>
          </div>
          <input value={indexSearch} onChange={(event) => setIndexSearch(event.target.value)} placeholder={tCurrent('auto.remoteSearchCluster.5n8wy2')} />
          <div className="search-index-list">
            {filteredIndices.map((index) => (
              <button
                key={index.index}
                type="button"
                className={`${selectedIndex?.index === index.index ? 'active' : ''} ${index.health || 'unknown'}`}
                onClick={() => {
                  setSelectedIndexName(index.index);
                  setQueryIndex(index.index);
                }}
              >
                <strong>{index.index}</strong>
                <span>{index.docsCount.toLocaleString(getShellDeskLocale())} docs · {index.storeSize || '-'}</span>
                <em>{index.health || 'unknown'} · {index.status || '-'}</em>
              </button>
            ))}
            {!filteredIndices.length ? <div className="search-empty-state">{tCurrent('auto.remoteSearchCluster.yg87h8')}</div> : null}
          </div>
        </aside>

        <main className="search-main">
          <nav className="search-tabs">
            {[
              ['overview', tCurrent('auto.remoteSearchCluster.y4kz1z')],
              ['shards', tCurrent('auto.remoteSearchCluster.1y6wwq')],
              ['query', tCurrent('auto.remoteSearchCluster.16mfmhy')],
              ['raw', tCurrent('auto.remoteSearchCluster.nkv7uu')],
            ].map(([key, label]) => (
              <button key={key} type="button" className={activeTab === key ? 'active' : ''} onClick={() => setActiveTab(key as SearchTab)}>{label}</button>
            ))}
          </nav>

          {activeTab === 'overview' ? (
            <section className="search-overview">
              {[
                [tCurrent('auto.remoteSearchCluster.1osfy9g'), health?.number_of_nodes ?? '-'],
                [tCurrent('auto.remoteSearchCluster.1qktaq6'), health?.number_of_data_nodes ?? '-'],
                [tCurrent('auto.remoteSearchCluster.1lig4k02'), indices.length],
                [tCurrent('auto.remoteSearchCluster.ynqhux'), health?.active_primary_shards ?? '-'],
                [tCurrent('auto.remoteSearchCluster.esjvc0'), health?.active_shards ?? '-'],
                [tCurrent('auto.remoteSearchCluster.6ug088'), health?.relocating_shards ?? '-'],
                [tCurrent('auto.remoteSearchCluster.jllirh'), health?.initializing_shards ?? '-'],
                [tCurrent('auto.remoteSearchCluster.1bgs62s'), health?.unassigned_shards ?? '-'],
              ].map(([label, value]) => (
                <div key={label} className="search-metric">
                  <span>{label}</span>
                  <strong>{value}</strong>
                </div>
              ))}
              <div className="search-selected-index">
                <strong>{selectedIndex?.index ?? tCurrent('auto.remoteSearchCluster.1o14bk6')}</strong>
                <span>{selectedIndex ? `${selectedIndex.docsCount.toLocaleString(getShellDeskLocale())} docs · ${selectedIndex.storeSize || '-'}` : tCurrent('auto.remoteSearchCluster.11d842q')}</span>
              </div>
            </section>
          ) : null}

          {activeTab === 'shards' ? (
            <section className="search-table-wrap">
              <table className="search-table">
                <thead>
                  <tr>
                    <th>Index</th>
                    <th>Shard</th>
                    <th>Role</th>
                    <th>State</th>
                    <th>Docs</th>
                    <th>Store</th>
                    <th>Node</th>
                  </tr>
                </thead>
                <tbody>
                  {selectedIndexShards.map((shard, index) => (
                    <tr key={`${shard.index}-${shard.shard}-${shard.prirep}-${index}`}>
                      <td>{shard.index}</td>
                      <td>{shard.shard}</td>
                      <td>{shard.prirep}</td>
                      <td>{shard.state}</td>
                      <td>{shard.docs}</td>
                      <td>{shard.store}</td>
                      <td>{shard.node || shard.ip}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {!selectedIndexShards.length ? <div className="search-empty-state">{tCurrent('auto.remoteSearchCluster.j2lpcc')}</div> : null}
            </section>
          ) : null}

          {activeTab === 'query' ? (
            <section className="search-query">
              <div className="search-query-form">
                <label>
                  <span>Index</span>
                  <input value={queryIndex} onChange={(event) => setQueryIndex(event.target.value)} placeholder="logs-*" />
                </label>
                <div className="search-query-actions">
                  <button type="button" onClick={formatQueryBody}>{tCurrent('auto.remoteSearchCluster.1i126as')}</button>
                  <button type="button" className="primary" onClick={runSearch} disabled={queryRunning}>{queryRunning ? tCurrent('auto.remoteSearchCluster.q3j9w1') : tCurrent('auto.remoteSearchCluster.1fd3gsv')}</button>
                </div>
              </div>
              <div className="search-query-grid">
                <CodeMirror
                  className="search-query-editor"
                  value={queryBody}
                  height="100%"
                  basicSetup={{
                    lineNumbers: true,
                    foldGutter: true,
                    highlightActiveLine: true,
                    highlightActiveLineGutter: true,
                    bracketMatching: true,
                    closeBrackets: true,
                    autocompletion: true,
                    searchKeymap: true,
                    defaultKeymap: true,
                    history: true,
                  }}
                  extensions={queryEditorExtensions}
                  onChange={setQueryBody}
                />
                <pre>{queryResponse || tCurrent('auto.remoteSearchCluster.11tbyaa')}</pre>
              </div>
            </section>
          ) : null}

          {activeTab === 'raw' ? (
            <pre className="search-raw">{rawResponse || tCurrent('auto.remoteSearchCluster.1d9lm8t')}</pre>
          ) : null}
        </main>
      </div>
    </section>
  );
}

export default RemoteSearchCluster;

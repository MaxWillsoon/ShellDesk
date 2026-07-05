import React from 'react';
import { createRoot } from 'react-dom/client';

import RemoteMySQL from '../../src/components/remote-desktop/RemoteMySQL';
import RemoteRedis from '../../src/components/remote-desktop/RemoteRedis';
import { loadFullMessageCatalog } from '../../src/i18n';
import '../../src/styles/index.scss';

const connectionId = 'ui-test-connection';
const hostId = 'ui-test-host';
const now = new Date('2026-01-01T00:00:00Z').toISOString();

function createMysqlResult(columns: string[], rows: Record<string, unknown>[]) {
  return {
    columns,
    rows,
    rowCount: rows.length,
    affectedRows: 0,
  };
}

function installGuiSshMock() {
  const mysqlColumns = [
    { name: 'id', type: 'INT', nullable: false, key: 'PRI', default: null },
    { name: 'name', type: 'VARCHAR(64)', nullable: true, key: '', default: null },
  ];
  const redisKey = {
    name: 'demo:key',
    type: 'string',
    ttl: -1,
    size: 5,
    scannedAt: now,
  };

  (window as any).guiSSH = {
    connections: {
      mysqlConnect: async () => ({ mysqlId: 'mysql-ui-test', transport: 'tunnel' }),
      mysqlDisconnect: async () => true,
      mysqlDatabases: async () => ['test'],
      mysqlTables: async () => ['users'],
      mysqlColumns: async () => mysqlColumns,
      mysqlQuery: async (_connectionId: string, _mysqlId: string, sql: string) => {
        if (/^CREATE\s+TABLE/i.test(sql.trim())) {
          throw new Error('mock create table failure');
        }
        return createMysqlResult(['id', 'name'], [{ id: 1, name: 'Alice' }]);
      },
      mysqlUpdateCell: async () => {
        throw new Error('mock cell update failure');
      },

      redisConnect: async () => ({ redisId: 'redis-ui-test', transport: 'tunnel' }),
      redisDisconnect: async () => true,
      redisScan: async () => ({
        cursor: '0',
        complete: true,
        pattern: '*',
        scannedAt: now,
        keys: [redisKey],
      }),
      redisGetValue: async () => ({
        type: 'string',
        value: 'hello',
        ttl: -1,
        size: 5,
      }),
      redisSetValue: async () => true,
      redisDeleteKey: async () => {
        throw new Error('mock redis delete failure');
      },
      redisRemoveListItem: async () => true,
      redisCommand: async () => 'OK',
    },
    vault: {
      getRemoteConnectionProfile: async () => null,
      saveRemoteConnectionProfile: async () => null,
    },
    events: {
      onDatabaseTunnelIdleTimeout: () => () => undefined,
    },
  };
}

function App() {
  const params = new URLSearchParams(window.location.search);
  const component = params.get('component') ?? 'mysql';

  if (component === 'redis') {
    return <RemoteRedis connectionId={connectionId} hostId={hostId} />;
  }

  return <RemoteMySQL connectionId={connectionId} hostId={hostId} />;
}

installGuiSshMock();
document.documentElement.setAttribute('data-language', 'zh-CN');
await loadFullMessageCatalog();

createRoot(document.getElementById('root')!).render(<App />);

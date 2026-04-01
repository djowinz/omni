import React from 'react';
import { ConnectionStatus } from '../components/ConnectionStatus';
import { useHostStatus } from '../hooks/useHostStatus';

export default function Home() {
  const status = useHostStatus();

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100vh',
      background: '#0f0f1a',
      fontFamily: 'system-ui, -apple-system, sans-serif',
    }}>
      <div style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}>
        <ConnectionStatus
          connected={status.connected}
          activeOverlay={status.activeOverlay}
          injectedGame={status.injectedGame}
        />
      </div>
      <div style={{
        background: '#1a1a2e',
        borderTop: '1px solid #2a2a40',
        padding: '6px 16px',
        fontSize: 11,
        color: '#505060',
      }}>
        v0.1.0
      </div>
    </div>
  );
}

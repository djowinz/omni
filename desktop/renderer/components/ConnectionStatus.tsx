import React from 'react';

interface Props {
  connected: boolean;
  activeOverlay?: string;
  injectedGame?: string;
}

export function ConnectionStatus({ connected, activeOverlay, injectedGame }: Props) {
  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      gap: 12,
    }}>
      <div style={{
        width: 10,
        height: 10,
        borderRadius: '50%',
        background: connected ? '#4ade80' : '#ef4444',
        animation: connected ? undefined : 'pulse 2s infinite',
      }} />
      <span style={{ color: '#c0c0d0', fontSize: 14 }}>
        {connected ? 'Connected to host' : 'Connecting to host...'}
      </span>
      {connected && activeOverlay && (
        <span style={{ color: '#505060', fontSize: 12 }}>
          Active overlay: {activeOverlay}
        </span>
      )}
      {connected && injectedGame && (
        <span style={{ color: '#505060', fontSize: 12 }}>
          Injected: {injectedGame}
        </span>
      )}
      {!connected && (
        <span style={{ color: '#505060', fontSize: 12 }}>
          Retrying...
        </span>
      )}
    </div>
  );
}

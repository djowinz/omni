import { useState, useEffect } from 'react';

interface HostStatus {
  connected: boolean;
  activeOverlay?: string;
  injectedGame?: string;
}

declare global {
  interface Window {
    omni?: {
      onHostStatus: (callback: (status: HostStatus) => void) => void;
    };
  }
}

export function useHostStatus(): HostStatus {
  const [status, setStatus] = useState<HostStatus>({ connected: false });

  useEffect(() => {
    window.omni?.onHostStatus((newStatus) => {
      setStatus(newStatus);
    });
  }, []);

  return status;
}

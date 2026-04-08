import { useState, useEffect } from 'react';
import type { SensorSnapshot } from '@/generated/SensorSnapshot';
import { BackendApi } from '@/lib/backend-api';

const backend = new BackendApi();

/** Hook that subscribes to live sensor data from the host. */
export function useSensorData(): SensorSnapshot | null {
  const [snapshot, setSnapshot] = useState<SensorSnapshot | null>(null);

  useEffect(() => {
    // Subscribe to sensor stream from host
    backend.subscribeSensors().catch(() => {
      // Host not connected yet — will retry when preview panel re-renders
    });

    const unsub = window.omni?.onSensorData?.((data) => {
      setSnapshot(data as SensorSnapshot);
    });
    return () => {
      unsub?.();
    };
  }, []);

  return snapshot;
}

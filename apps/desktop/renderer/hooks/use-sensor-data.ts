import { useState, useEffect } from 'react';
import type { SensorSnapshot } from '@omni/shared-types';
import type { HwInfoData } from '@/lib/sensor-mapping';
import { BackendApi } from '@/lib/backend-api';

const backend = new BackendApi();

export interface SensorData {
  snapshot: SensorSnapshot;
  hwinfo?: HwInfoData;
}

/** Hook that subscribes to live sensor data from the host. */
export function useSensorData(): SensorData | null {
  const [data, setData] = useState<SensorData | null>(null);

  useEffect(() => {
    // Subscribe to sensor stream from host
    backend.subscribeSensors().catch(() => {
      // Host not connected yet — will retry when preview panel re-renders
    });

    const unsub = window.omni?.onSensorData?.((msg) => {
      // msg may be the old shape (bare SensorSnapshot) or the new shape { snapshot, hwinfo }
      if (msg && 'snapshot' in msg) {
        setData({
          snapshot: msg.snapshot as SensorSnapshot,
          hwinfo: msg.hwinfo as HwInfoData | undefined,
        });
      } else {
        setData({ snapshot: msg as SensorSnapshot });
      }
    });
    return () => {
      unsub?.();
    };
  }, []);

  return data;
}

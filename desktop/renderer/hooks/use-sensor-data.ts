import { useState, useEffect } from 'react';
import type { SensorSnapshot } from '@/src/generated/SensorSnapshot';

/** Hook that subscribes to live sensor data from the host. */
export function useSensorData(): SensorSnapshot | null {
  const [snapshot, setSnapshot] = useState<SensorSnapshot | null>(null);

  useEffect(() => {
    window.omni?.onSensorData?.((data) => {
      setSnapshot(data as SensorSnapshot);
    });
  }, []);

  return snapshot;
}

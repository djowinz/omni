import { useMemo } from 'react';
import { BackendApi } from '@/lib/backend-api';

/** Singleton BackendApi hook. */
export function useBackend(): BackendApi {
  return useMemo(() => new BackendApi(), []);
}

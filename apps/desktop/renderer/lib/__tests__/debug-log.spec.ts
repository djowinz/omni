import { beforeEach, describe, expect, it, vi } from 'vitest';

describe('debugLog', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllEnvs();
    // jsdom test env provides window + localStorage
    window.localStorage.clear();
  });

  it('no-ops when both gates are off', async () => {
    vi.stubEnv('NEXT_PUBLIC_OMNI_DEBUG', '');
    const spy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const { debugLog } = await import('../debug-log');
    debugLog('should not appear', { a: 1 });
    expect(spy).not.toHaveBeenCalled();
  });

  it('delegates to console.log when NEXT_PUBLIC_OMNI_DEBUG=1', async () => {
    vi.stubEnv('NEXT_PUBLIC_OMNI_DEBUG', '1');
    const spy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const { debugLog } = await import('../debug-log');
    debugLog('env enabled', 42);
    expect(spy).toHaveBeenCalledWith('env enabled', 42);
  });

  it('delegates to console.log when localStorage OMNI_DEBUG=1', async () => {
    vi.stubEnv('NEXT_PUBLIC_OMNI_DEBUG', '');
    window.localStorage.setItem('OMNI_DEBUG', '1');
    const spy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const { debugLog } = await import('../debug-log');
    debugLog('localStorage enabled');
    expect(spy).toHaveBeenCalledWith('localStorage enabled');
  });
});

import { describe, expect, it, vi } from 'vitest';
import { makeDebugLog } from '../debug-log';

describe('makeDebugLog', () => {
  it('returns a no-op when OMNI_DEBUG is unset', () => {
    const spy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const log = makeDebugLog({});
    log('should be silent', { k: 'v' });
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });

  it('returns a no-op when OMNI_DEBUG is empty string', () => {
    const spy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const log = makeDebugLog({ OMNI_DEBUG: '' });
    log('silent');
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });

  it('delegates to console.log when OMNI_DEBUG=1', () => {
    const spy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const log = makeDebugLog({ OMNI_DEBUG: '1' });
    log('loud', 99);
    expect(spy).toHaveBeenCalledWith('loud', 99);
    spy.mockRestore();
  });

  it('ignores non-"1" OMNI_DEBUG values (treat only "1" as true)', () => {
    const spy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const log = makeDebugLog({ OMNI_DEBUG: 'true' });
    log('should be silent');
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });
});

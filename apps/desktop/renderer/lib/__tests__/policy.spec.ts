import { describe, it, expect } from 'vitest';
import { POLICY_ALLOWED, POLICY_NOT_ALLOWED } from '../policy';

describe('policy arrays', () => {
  it('POLICY_ALLOWED has at least one non-empty string entry', () => {
    expect(POLICY_ALLOWED.length).toBeGreaterThanOrEqual(1);
    for (const entry of POLICY_ALLOWED) {
      expect(typeof entry).toBe('string');
      expect(entry.trim().length).toBeGreaterThan(0);
    }
  });

  it('POLICY_NOT_ALLOWED has at least one non-empty string entry', () => {
    expect(POLICY_NOT_ALLOWED.length).toBeGreaterThanOrEqual(1);
    for (const entry of POLICY_NOT_ALLOWED) {
      expect(typeof entry).toBe('string');
      expect(entry.trim().length).toBeGreaterThan(0);
    }
  });
});

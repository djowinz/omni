import { describe, it, expect, vi, beforeEach } from 'vitest';
import { toast as sonnerToast } from 'sonner';
import { toast } from '../toast';
import { mapErrorToUserMessage, type OmniError } from '../map-error-to-user-message';

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

const writeText = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  writeText.mockReset();
  vi.stubGlobal('navigator', { clipboard: { writeText } });
});

describe('toast wrapper', () => {
  it('success() passes through to sonnerToast.success', () => {
    toast.success('hi');
    expect(sonnerToast.success).toHaveBeenCalledTimes(1);
    expect(sonnerToast.success).toHaveBeenCalledWith('hi');
  });

  it('info() passes through to sonnerToast.info', () => {
    toast.info('hi');
    expect(sonnerToast.info).toHaveBeenCalledTimes(1);
    expect(sonnerToast.info).toHaveBeenCalledWith('hi');
  });

  it("error() calls sonnerToast.error with mapped.text and a 'Report this' action", () => {
    const err: OmniError = {
      code: 'E_AUTH_001',
      kind: 'Auth',
      detail: 'server-internal-detail-sentinel',
      message: 'You are not signed in.',
    };
    const mapped = mapErrorToUserMessage(err);

    toast.error(err);

    expect(sonnerToast.error).toHaveBeenCalledTimes(1);
    const [text, opts] = (sonnerToast.error as unknown as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(text).toBe(mapped.text);
    expect(text).toBe(err.message);
    expect(opts).toBeDefined();
    expect(opts.action).toBeDefined();
    expect(opts.action.label).toBe('Report this');
    expect(typeof opts.action.onClick).toBe('function');
  });

  it('error() action.onClick writes mapped.opaquePayload to the clipboard', () => {
    const err: OmniError = {
      code: 'E_INTEGRITY_009',
      kind: 'Integrity',
      detail: 'hash mismatch',
      message: 'Bundle failed integrity check.',
    };
    const mapped = mapErrorToUserMessage(err);

    toast.error(err);

    const [, opts] = (sonnerToast.error as unknown as ReturnType<typeof vi.fn>).mock.calls[0];
    opts.action.onClick();

    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText).toHaveBeenCalledWith(mapped.opaquePayload);

    // Envelope shape sanity check — stable JSON with {code, kind, detail}.
    expect(JSON.parse(mapped.opaquePayload)).toEqual({
      code: err.code,
      kind: err.kind,
      detail: err.detail,
    });
  });

  it('error() handles OmniError without detail (detail serialized as null)', () => {
    const err: OmniError = {
      code: 'E_IO_002',
      kind: 'Io',
      message: 'Network hiccup.',
    };
    const mapped = mapErrorToUserMessage(err);

    toast.error(err);

    const [, opts] = (sonnerToast.error as unknown as ReturnType<typeof vi.fn>).mock.calls[0];
    opts.action.onClick();

    expect(writeText).toHaveBeenCalledWith(mapped.opaquePayload);
    expect(JSON.parse(mapped.opaquePayload)).toEqual({
      code: err.code,
      kind: err.kind,
      detail: null,
    });
  });
});

import { useMemo, useState, useEffect } from 'react';
import { Pencil } from 'lucide-react';
import { useForm } from 'react-hook-form';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useIdentity } from '../../lib/identity-context';
import { useShareWs } from '../../hooks/use-share-ws';
import { mapErrorToUserMessage, type OmniError } from '../../lib/map-error-to-user-message';

const MAX_CODEPOINTS = 32;

function codepointLength(s: string): number {
  return [...s].length;
}

function validate(value: string): string | null {
  const normalized = value.normalize('NFC').trim();
  if (normalized.length === 0) {
    return 'Display name must be 1–32 characters after trim.';
  }
  const len = codepointLength(normalized);
  if (len > MAX_CODEPOINTS) {
    return 'Display name must be 1–32 characters after trim.';
  }
  for (const ch of normalized) {
    const code = ch.codePointAt(0);
    if (code === undefined) continue;
    if (code < 0x20 || (code >= 0x7f && code <= 0x9f)) {
      return 'Display name contains control characters.';
    }
    if (code >= 0xd800 && code <= 0xdfff) {
      return 'Display name contains surrogate code points.';
    }
  }
  return null;
}

interface FormValues {
  display_name: string;
}

export function DisplayNameField() {
  const { identity, refresh } = useIdentity();
  const { send } = useShareWs();
  const [editing, setEditing] = useState(false);
  const [serverError, setServerError] = useState<string | null>(null);

  const { register, watch, handleSubmit, reset } = useForm<FormValues>({
    defaultValues: { display_name: identity?.display_name ?? '' },
  });

  // Keep RHF default in sync if identity loads after mount.
  useEffect(() => {
    if (!editing) {
      reset({ display_name: identity?.display_name ?? '' });
    }
  }, [identity?.display_name, editing, reset]);

  const value = watch('display_name');
  const localError = useMemo(() => validate(value), [value]);
  const slice = identity?.pubkey_hex.slice(0, 8) ?? '';
  const codepoints = codepointLength(value.normalize('NFC').trim());

  const enterEdit = () => {
    setServerError(null);
    reset({ display_name: identity?.display_name ?? '' });
    setEditing(true);
  };

  const cancel = () => {
    setEditing(false);
    setServerError(null);
  };

  const onSubmit = handleSubmit(async ({ display_name }) => {
    if (localError) return;
    const normalized = display_name.normalize('NFC').trim();
    try {
      await send('identity.setDisplayName', { display_name: normalized });
      await refresh();
      setEditing(false);
    } catch (err) {
      setServerError(mapErrorToUserMessage(err as OmniError).text);
    }
  });

  if (!editing) {
    const current = identity?.display_name;
    return (
      <div className="flex items-center justify-between py-1.5">
        <span className="text-[10px] uppercase tracking-wider text-[#52525b]">Display name</span>
        <div className="flex items-center gap-1.5">
          {current ? (
            <span className="text-[13px] font-medium text-[#fafafa]">{current}</span>
          ) : (
            <span className="text-[13px] italic text-[#52525b]">Set a display name</span>
          )}
          <button
            type="button"
            onClick={enterEdit}
            aria-label="Edit display name"
            className="rounded p-1 text-[#71717a] hover:bg-[#27272a]/50 hover:text-[#a1a1aa]"
          >
            <Pencil className="h-3 w-3" />
          </button>
        </div>
      </div>
    );
  }

  const counterClass =
    codepoints >= 33
      ? 'text-[#ef4444]'
      : codepoints >= 28
        ? 'text-[#fbbf24]'
        : 'text-[#52525b]';

  return (
    <form onSubmit={onSubmit} className="py-1.5">
      <div className="mb-1.5 flex items-center justify-between">
        <Label
          htmlFor="display-name-input"
          className="text-[10px] uppercase tracking-wider text-[#52525b]"
        >
          Display name
        </Label>
        <span className={`font-mono text-[10px] ${counterClass}`}>{codepoints} / 32</span>
      </div>
      <Input
        id="display-name-input"
        autoFocus
        {...register('display_name')}
        className={localError ? 'border-[#ef4444]' : undefined}
      />
      {!localError && value.trim().length > 0 ? (
        <div className="mt-2 rounded-md border border-dashed border-[#27272a] bg-[#0d0d0f] p-2 font-mono">
          <div className="text-[9px] uppercase tracking-wider text-[#52525b]">
            How others will see you
          </div>
          <div className="text-[12px] text-[#fafafa]">
            {`${value.normalize('NFC').trim()}#${slice}`}
          </div>
        </div>
      ) : null}
      {(localError || serverError) && (
        <p className="mt-2 flex items-start gap-1.5 text-[11px] text-[#ef4444]" role="alert">
          {localError ?? serverError}
        </p>
      )}
      <div className="mt-2 flex gap-1.5">
        <Button type="button" variant="outline" size="sm" onClick={cancel}>
          Cancel
        </Button>
        <Button type="submit" size="sm" disabled={localError !== null}>
          Save
        </Button>
      </div>
    </form>
  );
}

import { useMemo, useState } from 'react';
import { ArrowRight, GitFork } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

export type ForkSourceKind = 'remote' | 'local';

export interface ForkDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  sourceKind: ForkSourceKind;
  origin: { name: string; author_handle: string };
  defaultName: string;
  selfHandle: string;
  existingNames: string[];
  onFork: (input: { target_name: string }) => void;
  onCancel: () => void;
}

const FORBIDDEN_CHARS = ['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'];

// Mirrors crates/host/src/workspace/fork.rs — WINDOWS_RESERVED_STEMS constant.
const WINDOWS_RESERVED_STEMS = new Set([
  'CON',
  'PRN',
  'AUX',
  'NUL',
  'COM0',
  'COM1',
  'COM2',
  'COM3',
  'COM4',
  'COM5',
  'COM6',
  'COM7',
  'COM8',
  'COM9',
  'LPT0',
  'LPT1',
  'LPT2',
  'LPT3',
  'LPT4',
  'LPT5',
  'LPT6',
  'LPT7',
  'LPT8',
  'LPT9',
]);

/**
 * Mirrors crates/host/src/workspace/fork.rs::sanitize_name byte-for-byte.
 *
 * Rules (in order):
 *  1. name must not be empty
 *  2. char count (Unicode scalar values) must not exceed 48
 *  3. no leading/trailing whitespace
 *  4. not "." or ".."
 *  5. no forbidden path characters or NUL byte: / \ : * ? " < > |
 *  6. no control characters (codePoint < 0x20)
 *  7. stem (part before first '.') must not be a Windows reserved name
 *  8. case-insensitive collision with existingNames
 *
 * Error messages match the Rust source exactly so that host and renderer agree.
 */
function validateName(name: string, existingNames: string[]): string | null {
  if (name.length === 0) {
    return 'name must not be empty';
  }
  // Unicode scalar value count (matches Rust `str::chars().count()`).
  if ([...name].length > 48) {
    return 'name exceeds 48 characters';
  }
  if (name !== name.trim()) {
    return 'name must not have leading or trailing whitespace';
  }
  if (name === '.' || name === '..') {
    return "name must not be '.' or '..'";
  }
  for (const ch of name) {
    if (FORBIDDEN_CHARS.includes(ch)) {
      return 'name contains a forbidden character';
    }
    const code = ch.codePointAt(0);
    // Mirrors Rust `c.is_control()` for the ASCII/C0 range checked here.
    if (code !== undefined && code < 0x20) {
      return 'name contains a non-printable character';
    }
  }
  // Stem = part before first '.', case-insensitive comparison against reserved list.
  // Mirrors: `name.split('.').next().unwrap_or(name)` + `to_ascii_uppercase()`.
  const stem = name.split('.')[0] ?? name;
  if (WINDOWS_RESERVED_STEMS.has(stem.toUpperCase())) {
    return 'name is a Windows reserved stem';
  }
  if (existingNames.some((existing) => existing.toLowerCase() === name.toLowerCase())) {
    return 'a workspace with that name already exists';
  }
  return null;
}

export function ForkDialog({
  open,
  onOpenChange,
  sourceKind,
  origin,
  defaultName,
  selfHandle,
  existingNames,
  onFork,
  onCancel,
}: ForkDialogProps) {
  const [name, setName] = useState(defaultName);
  const error = useMemo(() => validateName(name, existingNames), [name, existingNames]);

  const sourceLabel = sourceKind === 'remote' ? 'From · Remote' : 'From · Installed';
  const sourceBorderClass =
    sourceKind === 'remote'
      ? 'border-[#3b82f6]/40 bg-[#3b82f6]/[0.06]'
      : 'border-[#22c55e]/40 bg-[#22c55e]/[0.06]';
  const sourceLabelClass = sourceKind === 'remote' ? 'text-[#3b82f6]' : 'text-[#22c55e]';

  const submit = () => {
    if (error) return;
    onFork({ target_name: name });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[560px] overflow-hidden p-0">
        <DialogHeader className="space-y-0 border-b border-[#27272a] px-5 py-4">
          <DialogTitle className="flex items-center gap-2.5 text-[15px] text-[#fafafa]">
            <GitFork className="h-5 w-5 text-[#a1a1aa]" />
            Fork to local copy
          </DialogTitle>
          <DialogDescription className="ml-7 mt-1 text-[11px] text-[#a1a1aa]">
            Make an editable copy you own. Changes won&apos;t affect the original.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 bg-[#0d0d0f] p-5">
          <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-3">
            {/* Origin node */}
            <div className={`rounded-md border p-2.5 ${sourceBorderClass}`}>
              <div
                className={`mb-1 text-[9px] font-semibold uppercase tracking-wider ${sourceLabelClass}`}
              >
                {sourceLabel}
              </div>
              <div className="font-mono text-[13px] font-medium text-[#fafafa]">
                {origin.name}
              </div>
              <div className="mt-0.5 font-mono text-[11px] text-[#a1a1aa]">
                {origin.author_handle}
              </div>
            </div>

            <ArrowRight className="h-5 w-5 text-[#a1a1aa]" />

            {/* Target node — always green-bordered */}
            <div className="rounded-md border border-[#22c55e]/40 bg-[#22c55e]/[0.06] p-2.5">
              <div className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-[#22c55e]">
                To · Local copy
              </div>
              <div className="truncate font-mono text-[13px] font-medium text-[#fafafa]">
                {name || ' '}
              </div>
              <div className="mt-0.5 font-mono text-[11px] text-[#a1a1aa]">
                you · {selfHandle}
              </div>
            </div>
          </div>

          {/* Name input */}
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="fork-name" className="text-xs text-[#fafafa]">
              New workspace name
            </Label>
            <Input
              id="fork-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
            />
            <p className="text-[11px] leading-relaxed text-[#a1a1aa]">
              {error ? (
                <span className="text-[#ef4444]">{error}</span>
              ) : (
                <>
                  Up to 48 characters. Spaces, punctuation, and emoji are fine. Avoid{' '}
                  <code className="font-mono text-[10px] text-[#fafafa]">
                    {'/ \\ : * ? " < > |'}
                  </code>{' '}
                  and reserved Windows names like{' '}
                  <code className="font-mono text-[10px] text-[#fafafa]">CON</code>,{' '}
                  <code className="font-mono text-[10px] text-[#fafafa]">AUX</code>,{' '}
                  <code className="font-mono text-[10px] text-[#fafafa]">NUL</code>.
                </>
              )}
            </p>
          </div>
        </div>

        <DialogFooter className="border-t border-[#27272a] bg-[#18181b] px-5 py-3.5">
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={error !== null}>
            Fork
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

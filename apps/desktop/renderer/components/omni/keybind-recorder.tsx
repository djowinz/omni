import { useState, useCallback, useEffect, useRef } from 'react';
import { Kbd } from '@/components/ui/kbd';
import { cn } from '@/lib/utils';

const MODIFIER_KEYS = ['Control', 'Alt', 'Shift', 'Meta'];

interface KeybindRecorderProps {
  value: string;
  onChange: (key: string) => void;
}

/** Converts a KeyboardEvent into a display string like "Ctrl+Shift+F9". */
function eventToKeyString(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');

  // Don't include bare modifier keys as the main key
  if (!MODIFIER_KEYS.includes(e.key)) {
    // Normalize key display name
    let key = e.key;
    if (key === ' ') key = 'Space';
    if (key.length === 1) key = key.toUpperCase();
    parts.push(key);
  }

  return parts.join('+');
}

export function KeybindRecorder({ value, onChange }: KeybindRecorderProps) {
  const [recording, setRecording] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === 'Escape') {
        setRecording(false);
        return;
      }

      // Ignore bare modifier presses — wait for a non-modifier key
      if (MODIFIER_KEYS.includes(e.key)) return;

      const keyString = eventToKeyString(e);
      if (keyString) {
        onChange(keyString);
        setRecording(false);
      }
    },
    [onChange],
  );

  useEffect(() => {
    if (recording) {
      window.addEventListener('keydown', handleKeyDown, true);
      return () => window.removeEventListener('keydown', handleKeyDown, true);
    }
  }, [recording, handleKeyDown]);

  // Split value for multi-key display, e.g. "Ctrl+F9" → ["Ctrl", "F9"]
  const keyParts = value.split('+');

  return (
    <div ref={containerRef} className="flex flex-col gap-1.5">
      {recording ? (
        <button
          onClick={() => setRecording(false)}
          className={cn(
            'inline-flex items-center justify-center rounded border border-[#00D9FF] border-b-2 border-b-[#00D9FF] bg-[#0D0D0F] px-3 py-1',
            'font-mono text-xs text-[#00D9FF] animate-pulse',
          )}
        >
          Press a key...
        </button>
      ) : (
        <button onClick={() => setRecording(true)} className="inline-flex items-center gap-1 group">
          {keyParts.map((part, i) => (
            <span key={i} className="contents">
              {i > 0 && <span className="text-[#52525B] text-[10px]">+</span>}
              <Kbd className="group-hover:border-[#00D9FF]/50 transition-colors">{part}</Kbd>
            </span>
          ))}
        </button>
      )}
      <span className="text-[10px] text-[#52525B]">
        {recording ? 'ESC to cancel' : 'Click to rebind'}
      </span>
    </div>
  );
}

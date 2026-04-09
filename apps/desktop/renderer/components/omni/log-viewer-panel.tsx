import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { X, Search, ChevronDown } from 'lucide-react';
import { useRouter } from 'next/router';
import { parseLogLine, type LogLevel, type ParsedLogLine } from '@/lib/log-parser';
import { cn } from '@/lib/utils';

const LEVEL_COLORS: Record<LogLevel, string> = {
  TRACE: 'text-[#71717A]',
  DEBUG: 'text-[#22C55E]',
  INFO: 'text-[#3B82F6]',
  WARN: 'text-[#EAB308]',
  ERROR: 'text-[#EF4444]',
};

const ALL_LEVELS: LogLevel[] = ['TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR'];

export function LogViewerPanel() {
  const router = useRouter();
  const [lines, setLines] = useState<ParsedLogLine[]>([]);
  const [search, setSearch] = useState('');
  const [enabledLevels, setEnabledLevels] = useState<Set<LogLevel>>(new Set(ALL_LEVELS));
  const [levelDropdownOpen, setLevelDropdownOpen] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const [tailing, setTailing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const parentRef = useRef<HTMLDivElement>(null);
  const levelDropdownRef = useRef<HTMLDivElement>(null);

  // Start/stop tailing on mount/unmount
  useEffect(() => {
    let unsubData: (() => void) | undefined;
    let unsubError: (() => void) | undefined;

    unsubData = window.omni?.onLogData((newLines: string[]) => {
      const parsed = newLines.map(parseLogLine);
      setLines((prev) => {
        const next = [...prev, ...parsed];
        return next.length > 50000 ? next.slice(next.length - 50000) : next;
      });
    });

    unsubError = window.omni?.onLogError((message: string) => {
      console.error('[log-viewer]', message);
      setError(message);
    });

    window.omni?.startLogTail().then(() => {
      setTailing(true);
      setError(null);
    }).catch((err: Error) => {
      setTailing(false);
      setError(err?.message ?? 'Failed to start log tailing');
      console.error('[log-viewer] startLogTail failed:', err);
    });

    return () => {
      window.omni?.stopLogTail();
      unsubData?.();
      unsubError?.();
      setTailing(false);
    };
  }, []);

  // Filtered lines by level, with search match flagging
  const { filteredLines, matchCount } = useMemo(() => {
    const searchLower = search.toLowerCase();
    let count = 0;
    const filtered = lines.filter((line) => {
      if (line.level && !enabledLevels.has(line.level)) return false;
      return true;
    });
    if (search) {
      for (const line of filtered) {
        if (line.raw.toLowerCase().includes(searchLower)) count++;
      }
    }
    return { filteredLines: filtered, matchCount: count };
  }, [lines, search, enabledLevels]);

  const virtualizer = useVirtualizer({
    count: filteredLines.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 24,
    overscan: 20,
  });

  // Auto-scroll to bottom when new lines arrive
  useEffect(() => {
    if (autoScroll && filteredLines.length > 0) {
      virtualizer.scrollToIndex(filteredLines.length - 1, { align: 'end' });
    }
  }, [filteredLines.length, autoScroll, virtualizer]);

  // Close level dropdown when clicking outside
  useEffect(() => {
    if (!levelDropdownOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (levelDropdownRef.current && !levelDropdownRef.current.contains(e.target as Node)) {
        setLevelDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [levelDropdownOpen]);

  // Detect manual scroll-up to pause auto-scroll
  const handleScroll = useCallback(() => {
    const el = parentRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
    setAutoScroll(atBottom);
  }, []);

  const toggleLevel = (level: LogLevel) => {
    setEnabledLevels((prev) => {
      const next = new Set(prev);
      if (next.has(level)) {
        next.delete(level);
      } else {
        next.add(level);
      }
      return next;
    });
  };

  const handleClose = () => {
    router.push('/home');
  };

  const handleJumpToLatest = () => {
    setAutoScroll(true);
    virtualizer.scrollToIndex(filteredLines.length - 1, { align: 'end' });
  };

  return (
    <div className="flex h-full flex-col bg-[#0A0A0C]">
      {/* Toolbar */}
      <div className="flex items-center justify-between border-b border-[#27272A] px-4 py-3">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold uppercase tracking-wider text-[#A1A1AA]">Service Logs</span>
          {tailing && (
            <span className="flex items-center gap-1 text-[10px] text-[#22C55E]">
              <span className="h-1.5 w-1.5 rounded-full bg-[#22C55E] animate-pulse" />
              Live
            </span>
          )}
          {error && (
            <span className="flex items-center gap-1 text-[10px] text-[#EF4444]">
              <span className="h-1.5 w-1.5 rounded-full bg-[#EF4444]" />
              {error}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {/* Search */}
          <div className="flex items-center gap-1 rounded border border-[#27272A] bg-[#18181B] px-1.5 h-6">
            <Search className="h-3 w-3 text-[#52525B] flex-shrink-0" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search..."
              className="w-32 bg-transparent text-[10px] text-[#A1A1AA] placeholder-[#52525B] outline-none"
            />
            {search && (
              <span className="text-[9px] text-[#52525B] flex-shrink-0">{matchCount}</span>
            )}
          </div>

          {/* Level filter */}
          <div className="relative" ref={levelDropdownRef}>
            <button
              onClick={() => setLevelDropdownOpen(!levelDropdownOpen)}
              className="flex items-center gap-1 rounded border border-[#27272A] bg-[#18181B] px-1.5 h-6 text-[10px] text-[#A1A1AA] hover:bg-[#27272A]"
            >
              Level
              <ChevronDown className="h-3 w-3" />
            </button>
            {levelDropdownOpen && (
              <div className="absolute right-0 top-full z-10 mt-1 rounded border border-[#27272A] bg-[#18181B] py-1 shadow-lg">
                {ALL_LEVELS.map((level) => (
                  <label
                    key={level}
                    className="flex cursor-pointer items-center gap-2 px-3 py-1 text-xs hover:bg-[#27272A]"
                  >
                    <input
                      type="checkbox"
                      checked={enabledLevels.has(level)}
                      onChange={() => toggleLevel(level)}
                      className="accent-[#00D9FF]"
                    />
                    <span className={LEVEL_COLORS[level]}>{level}</span>
                  </label>
                ))}
              </div>
            )}
          </div>

          {/* Close */}
          <button
            onClick={handleClose}
            className="rounded p-0.5 text-[#52525B] hover:bg-[#27272A] hover:text-[#A1A1AA]"
            title="Close log viewer"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      </div>

      {/* Log lines */}
      <div
        ref={parentRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-scroll overflow-x-auto font-mono text-xs"
      >
        <div
          style={{
            height: `${virtualizer.getTotalSize()}px`,
            width: '100%',
            position: 'relative',
          }}
        >
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const line = filteredLines[virtualRow.index];
            const isMatch = search ? line.raw.toLowerCase().includes(search.toLowerCase()) : false;
            return (
              <div
                key={virtualRow.index}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
                className={cn(
                  'flex items-center gap-3 px-4 select-text',
                  isMatch ? 'bg-[#EAB308]/10' : 'hover:bg-[#27272A]/30',
                )}
              >
                <span className="w-24 flex-shrink-0 text-[#52525B]">
                  {line.timestamp ? (line.timestamp.split('T')[1]?.replace('Z', '') ?? line.timestamp) : ''}
                </span>
                <span className={cn('w-12 flex-shrink-0 font-semibold', line.level ? LEVEL_COLORS[line.level] : 'text-[#52525B]')}>
                  {line.level ?? ''}
                </span>
                <span className="text-[#A1A1AA] min-w-0 whitespace-pre">{line.message}</span>
              </div>
            );
          })}
        </div>
      </div>

      {/* Jump to latest indicator */}
      {!autoScroll && (
        <button
          onClick={handleJumpToLatest}
          className="absolute bottom-12 left-1/2 -translate-x-1/2 rounded-full border border-[#27272A] bg-[#18181B] px-4 py-1.5 text-xs text-[#A1A1AA] shadow-lg hover:bg-[#27272A] transition-colors"
        >
          <ChevronDown className="mr-1 inline h-3 w-3" />
          Jump to latest
        </button>
      )}
    </div>
  );
}

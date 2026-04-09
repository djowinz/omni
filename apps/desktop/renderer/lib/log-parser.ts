export type LogLevel = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';

export interface ParsedLogLine {
  timestamp: string | null;
  level: LogLevel | null;
  message: string;
  raw: string;
}

// Matches tracing_subscriber default format:
// 2026-04-08T12:04:01.123Z  INFO omni_host::module: message text
const LOG_PATTERN = /^(\d{4}-\d{2}-\d{2}T[\d:.]+Z)\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+(.+)$/;

export function parseLogLine(line: string): ParsedLogLine {
  const match = line.match(LOG_PATTERN);
  if (!match) {
    return { timestamp: null, level: null, message: line, raw: line };
  }
  return {
    timestamp: match[1],
    level: match[2] as LogLevel,
    message: match[3],
    raw: line,
  };
}

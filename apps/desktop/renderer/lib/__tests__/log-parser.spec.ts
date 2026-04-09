import { describe, it, expect } from 'vitest';
import { parseLogLine, type ParsedLogLine } from '../log-parser';

describe('parseLogLine', () => {
  it('should parse a standard tracing log line', () => {
    const line =
      '2026-04-08T12:04:01.123Z  INFO omni_host::ws_server: WebSocket server listening on 127.0.0.1:9473';
    const result = parseLogLine(line);
    expect(result).toEqual({
      timestamp: '2026-04-08T12:04:01.123Z',
      level: 'INFO',
      message: 'omni_host::ws_server: WebSocket server listening on 127.0.0.1:9473',
      raw: line,
    });
  });

  it('should parse WARN level', () => {
    const line = '2026-04-08T12:04:02.000Z  WARN omni_host: CPU temperature sensor not available';
    const result = parseLogLine(line);
    expect(result.level).toBe('WARN');
  });

  it('should parse ERROR level', () => {
    const line = '2026-04-08T12:04:11.000Z ERROR omni_host::sensors: Failed to read GPU fan speed';
    const result = parseLogLine(line);
    expect(result.level).toBe('ERROR');
  });

  it('should parse DEBUG level', () => {
    const line = '2026-04-08T12:04:10.000Z DEBUG omni_host: Sensor poll cycle complete';
    const result = parseLogLine(line);
    expect(result.level).toBe('DEBUG');
  });

  it('should parse TRACE level', () => {
    const line = '2026-04-08T12:04:10.000Z TRACE omni_host: detailed trace info';
    const result = parseLogLine(line);
    expect(result.level).toBe('TRACE');
  });

  it('should handle lines that do not match the expected format', () => {
    const line = 'Some random output without a level';
    const result = parseLogLine(line);
    expect(result).toEqual({
      timestamp: null,
      level: null,
      message: line,
      raw: line,
    });
  });

  it('should handle lines with extra whitespace between fields', () => {
    const line = '2026-04-08T12:04:01.000Z   INFO  omni_host: message with spaces';
    const result = parseLogLine(line);
    expect(result.level).toBe('INFO');
    expect(result.timestamp).toBe('2026-04-08T12:04:01.000Z');
  });
});

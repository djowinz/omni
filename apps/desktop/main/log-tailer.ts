import fs from 'fs';
import { EventEmitter } from 'events';

/**
 * Tails a log file — reads full history then streams new lines as they're appended.
 * Emits 'lines' events with arrays of new complete lines.
 */
export class LogTailer extends EventEmitter {
  private filePath: string;
  private watcher: fs.FSWatcher | null = null;
  private offset = 0;
  private buffer = '';
  private reading = false;

  constructor(filePath: string) {
    super();
    this.filePath = filePath;
  }

  /** Start tailing — reads existing content then watches for changes. */
  start(): void {
    if (!fs.existsSync(this.filePath)) {
      this.emit('error', new Error(`Log file not found: ${this.filePath}`));
      return;
    }

    // Read existing content
    this.readFromOffset();

    // Watch for changes
    this.watcher = fs.watch(this.filePath, (eventType) => {
      if (eventType === 'change') {
        this.readFromOffset();
      }
    });

    this.watcher.on('error', (err) => {
      this.emit('error', err);
    });
  }

  /** Stop tailing — closes watcher and cleans up. */
  stop(): void {
    if (this.watcher) {
      this.watcher.close();
      this.watcher = null;
    }
    this.offset = 0;
    this.buffer = '';
    this.reading = false;
    this.removeAllListeners();
  }

  private readFromOffset(): void {
    // Prevent concurrent reads
    if (this.reading) return;
    this.reading = true;

    let stat: fs.Stats;
    try {
      stat = fs.statSync(this.filePath);
    } catch {
      this.reading = false;
      return;
    }

    // File was truncated or rotated — reset
    if (stat.size < this.offset) {
      this.offset = 0;
      this.buffer = '';
    }

    if (stat.size === this.offset) {
      this.reading = false;
      return;
    }

    const readSize = stat.size - this.offset;
    const buf = Buffer.alloc(readSize);
    let fd: number | null = null;

    try {
      fd = fs.openSync(this.filePath, 'r');
      fs.readSync(fd, buf, 0, readSize, this.offset);
      this.offset = stat.size;
    } catch {
      this.reading = false;
      return;
    } finally {
      if (fd !== null) fs.closeSync(fd);
    }

    this.reading = false;

    // Split into lines, keeping partial last line in buffer
    const raw = this.buffer + buf.toString('utf-8');
    const parts = raw.split('\n');
    this.buffer = parts.pop() ?? '';

    const lines = parts.filter((l) => l.length > 0);
    if (lines.length > 0) {
      this.emit('lines', lines);
    }
  }
}

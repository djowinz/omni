import { ChildProcess, spawn } from 'child_process';
import * as path from 'path';
import * as fs from 'fs';
import WebSocket from 'ws';
import { EventEmitter } from 'events';
import { app } from 'electron';

const WS_PORT = 9473;
const WS_URL = `ws://127.0.0.1:${WS_PORT}`;
const RECONNECT_INTERVAL_MS = 2000;

export interface HostStatus {
  connected: boolean;
  activeOverlay?: string;
  injectedGame?: string;
}

export class HostManager extends EventEmitter {
  private ws: WebSocket | null = null;
  private hostProcess: ChildProcess | null = null;
  private reconnectTimer: NodeJS.Timeout | null = null;
  private intentionalClose = false;
  private _status: HostStatus = { connected: false };

  get status(): HostStatus {
    return this._status;
  }

  async start(): Promise<void> {
    const connected = await this.tryConnect();
    if (!connected) {
      this.spawnHost();
      await new Promise(resolve => setTimeout(resolve, 1500));
      await this.tryConnect();
    }
  }

  private tryConnect(): Promise<boolean> {
    return new Promise(resolve => {
      try {
        const ws = new WebSocket(WS_URL);
        const timeout = setTimeout(() => {
          ws.close();
          resolve(false);
        }, 2000);

        ws.on('open', () => {
          clearTimeout(timeout);
          this.onConnected(ws);
          resolve(true);
        });

        ws.on('error', () => {
          clearTimeout(timeout);
          resolve(false);
        });
      } catch {
        resolve(false);
      }
    });
  }

  private onConnected(ws: WebSocket): void {
    this.ws = ws;
    this._status = { connected: true };
    this.emit('connected');
    this.send({ type: 'status' });

    ws.on('message', (data: Buffer) => {
      try {
        const msg = JSON.parse(data.toString());
        this.handleMessage(msg);
      } catch { /* ignore malformed */ }
    });

    ws.on('close', () => {
      if (!this.intentionalClose) {
        this._status = { connected: false };
        this.emit('disconnected');
        this.scheduleReconnect();
      }
    });

    ws.on('error', () => { /* close event will fire after this */ });
  }

  private handleMessage(msg: any): void {
    if (msg.type === 'status.data') {
      this._status = {
        connected: true,
        activeOverlay: msg.active_overlay,
        injectedGame: msg.injected_game,
      };
      this.emit('status', this._status);
    }
    this.emit('message', msg);
  }

  send(msg: object): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  /** Send a message and wait for a response with the matching type. */
  sendAndWait(msg: object, expectedType: string, timeoutMs = 5000): Promise<any> {
    return new Promise((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        reject(new Error('WebSocket not connected'));
        return;
      }

      const timer = setTimeout(() => {
        cleanup();
        reject(new Error(`Timeout waiting for ${expectedType}`));
      }, timeoutMs);

      const handler = (data: Buffer) => {
        try {
          const response = JSON.parse(data.toString());
          if (response.type === expectedType || response.type === 'error') {
            cleanup();
            if (response.type === 'error') {
              reject(new Error(response.message));
            } else {
              resolve(response);
            }
          }
        } catch { /* ignore malformed */ }
      };

      const cleanup = () => {
        clearTimeout(timer);
        this.ws?.removeListener('message', handler);
      };

      this.ws.on('message', handler);
      this.ws.send(JSON.stringify(msg));
    });
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return;
    this.reconnectTimer = setInterval(async () => {
      const connected = await this.tryConnect();
      if (connected && this.reconnectTimer) {
        clearInterval(this.reconnectTimer);
        this.reconnectTimer = null;
      }
    }, RECONNECT_INTERVAL_MS);
  }

  private spawnHost(): void {
    const hostPath = this.findHostExe();
    if (!hostPath) {
      this.emit('error', 'Could not find omni-host.exe');
      return;
    }

    const logDir = path.join(app.getPath('userData'), 'logs');
    if (!fs.existsSync(logDir)) {
      fs.mkdirSync(logDir, { recursive: true });
    }

    const logPath = path.join(logDir, 'omni-host.log');
    const logFd = fs.openSync(logPath, 'a');

    this.hostProcess = spawn(hostPath, ['--service'], {
      detached: true,
      stdio: ['ignore', logFd, logFd],
    });

    // Close the fd after spawn — the child process has its own copy
    fs.closeSync(logFd);

    this.hostProcess.on('exit', (code) => {
      if (!this.intentionalClose) {
        this.emit('host-crashed', code);
      }
      this.hostProcess = null;
    });

    this.hostProcess.unref();
  }

  private findHostExe(): string | null {
    // Installed layout: omni-host.exe next to Omni.exe
    const installedPath = path.join(path.dirname(app.getPath('exe')), 'omni-host.exe');
    if (fs.existsSync(installedPath)) return installedPath;

    // Dev layout: target/debug or target/release
    const devDebug = path.resolve(__dirname, '../../target/debug/omni-host.exe');
    if (fs.existsSync(devDebug)) return devDebug;

    const devRelease = path.resolve(__dirname, '../../target/release/omni-host.exe');
    if (fs.existsSync(devRelease)) return devRelease;

    return null;
  }

  async shutdown(): Promise<void> {
    this.intentionalClose = true;

    if (this.reconnectTimer) {
      clearInterval(this.reconnectTimer);
      this.reconnectTimer = null;
    }

    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }

    if (this.hostProcess) {
      this.hostProcess.kill();
      this.hostProcess = null;
    }
  }
}

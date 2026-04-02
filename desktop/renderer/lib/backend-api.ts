import type { OmniFile } from '@/src/generated/OmniFile';
import type { ParseError } from '@/src/generated/ParseError';
import type { Config } from '@/src/generated/Config';

/** Typed wrapper around window.omni.sendMessage for all backend operations. */
export class BackendApi {
  private send(msg: object): Promise<any> {
    if (!window.omni?.sendMessage) {
      return Promise.reject(new Error('IPC bridge not available'));
    }
    return window.omni.sendMessage(msg);
  }

  async listFiles(): Promise<any> {
    return this.send({ type: 'file.list' });
  }

  async readFile(path: string): Promise<string> {
    const res = await this.send({ type: 'file.read', path });
    return res.content ?? '';
  }

  async writeFile(path: string, content: string): Promise<void> {
    await this.send({ type: 'file.write', path, content });
  }

  async createOverlay(name: string): Promise<void> {
    await this.send({ type: 'file.create', createType: 'overlay', name });
  }

  async createTheme(name: string): Promise<void> {
    await this.send({ type: 'file.create', createType: 'theme', name });
  }

  async deleteFile(path: string): Promise<void> {
    await this.send({ type: 'file.delete', path });
  }

  async parseOverlay(source: string): Promise<{ file: OmniFile | null; diagnostics: ParseError[] }> {
    const res = await this.send({ type: 'widget.parse', source });
    return { file: res.file ?? null, diagnostics: res.diagnostics ?? [] };
  }

  async applyOverlay(source: string): Promise<{ file: OmniFile | null; diagnostics: ParseError[] }> {
    const res = await this.send({ type: 'widget.apply', source });
    return { file: res.file ?? null, diagnostics: res.diagnostics ?? [] };
  }

  async getConfig(): Promise<Config> {
    const res = await this.send({ type: 'config.get' });
    return res.config;
  }

  async updateConfig(config: Config): Promise<void> {
    await this.send({ type: 'config.update', config });
  }

  async getStatus(): Promise<any> {
    return this.send({ type: 'status' });
  }

  async subscribeSensors(): Promise<void> {
    await this.send({ type: 'sensors.subscribe' });
  }
}

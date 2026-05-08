import type { OmniFile, ParseError, Config } from '@omni/shared-types';

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

  async parseOverlay(
    source: string,
  ): Promise<{ file: OmniFile | null; diagnostics: ParseError[] }> {
    const res = await this.send({ type: 'widget.parse', source });
    return { file: res.file ?? null, diagnostics: res.diagnostics ?? [] };
  }

  async applyOverlay(
    source: string,
  ): Promise<{ file: OmniFile | null; diagnostics: ParseError[] }> {
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

  async subscribePreview(): Promise<{ active: boolean }> {
    const response = await this.send({ type: 'preview.subscribe' });
    return { active: response.active };
  }

  /**
   * Pin the editor preview stream to a specific overlay source.
   * The host parses `source`, builds initial HTML, and begins broadcasting
   * `preview.html.editor` / `preview.update.editor` frames for that overlay
   * independently of the in-game stream.
   */
  async setEditorOverlay(params: { source: string; overlay_name: string }): Promise<void> {
    await this.send({
      type: 'preview.setEditorOverlay',
      source: params.source,
      overlay_name: params.overlay_name,
    });
  }

  /**
   * Clear the pinned editor overlay, falling back to the mirror-by-default
   * path where the editor channel echoes the in-game stream.
   */
  async clearEditorOverlay(): Promise<void> {
    await this.send({ type: 'preview.clearEditorOverlay' });
  }
}

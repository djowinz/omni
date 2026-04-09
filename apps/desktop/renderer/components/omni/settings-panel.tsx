import { useState, useEffect } from 'react';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Settings, ChevronRight, RotateCw, ScrollText } from 'lucide-react';
import { useRouter } from 'next/router';
import { useOmniState } from '@/hooks/use-omni-state';
import { BackendApi } from '@/lib/backend-api';
import type { Config } from '@/generated/Config';
import { KeybindRecorder } from './keybind-recorder';
import { ProcessListDialog } from './process-list-dialog';
import { GameDirectoriesDialog } from './game-directories-dialog';
import { HwInfoSensorsDialog } from './hwinfo-sensors-dialog';

const backend = new BackendApi();

export function SettingsPanel() {
  const { state, dispatch } = useOmniState();
  const router = useRouter();
  const [excludeOpen, setExcludeOpen] = useState(false);
  const [includeOpen, setIncludeOpen] = useState(false);
  const [directoriesOpen, setDirectoriesOpen] = useState(false);
  const [hwinfoOpen, setHwinfoOpen] = useState(false);
  const [openAtLogin, setOpenAtLogin] = useState(false);
  const [restarting, setRestarting] = useState(false);

  // Load login item settings on mount
  useEffect(() => {
    window.omni?.getLoginItemSettings?.().then((settings) => {
      setOpenAtLogin(settings?.openAtLogin ?? false);
    });
  }, []);

  const config = state.config;
  const minimizeToTray = config?.minimize_to_tray ?? false;
  const toggleKeybind = config?.keybinds?.toggle_overlay ?? 'F12';
  const excludeList = config?.exclude ?? [];
  const includeList = config?.include ?? [];
  const gameDirectories = config?.game_directories ?? [];

  const updateConfig = async (partial: Partial<Config>) => {
    if (!config) return;
    const updated = { ...config, ...partial };
    await backend.updateConfig(updated);
    dispatch({ type: 'SET_CONFIG', payload: updated });
  };

  const handleToggleStartWithWindows = async (checked: boolean) => {
    setOpenAtLogin(checked);
    await window.omni?.setLoginItemSettings?.(checked);
  };

  const handleToggleMinimizeToTray = async (checked: boolean) => {
    await updateConfig({ minimize_to_tray: checked });
  };

  const handleKeybindChange = async (key: string) => {
    if (!config) return;
    const updated = {
      ...config,
      keybinds: { ...config.keybinds, toggle_overlay: key },
    };
    await backend.updateConfig(updated);
    dispatch({ type: 'SET_CONFIG', payload: updated });
  };

  const handleRestartHost = async () => {
    setRestarting(true);
    try {
      await window.omni?.restartHost?.();
    } finally {
      setTimeout(() => setRestarting(false), 3000);
    }
  };

  return (
    <>
      <div className="flex h-full flex-col">
        {/* Header */}
        <div className="flex items-center gap-2 border-b border-[#27272A] px-4 py-3">
          <Settings className="h-4 w-4 text-[#71717A]" />
          <span className="text-xs font-semibold uppercase tracking-wider text-[#A1A1AA]">
            Settings
          </span>
        </div>

        <ScrollArea className="flex-1">
          <div className="p-4 space-y-6">
            {/* General Section */}
            <section>
              <h3 className="text-[10px] font-semibold uppercase tracking-wider text-[#52525B] mb-3">
                General
              </h3>
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <Label className="text-xs text-[#A1A1AA]">Start with Windows</Label>
                  <Switch checked={openAtLogin} onCheckedChange={handleToggleStartWithWindows} />
                </div>
                <div className="flex items-center justify-between">
                  <div>
                    <Label className="text-xs text-[#A1A1AA]">Minimize to Tray</Label>
                    <p className="text-[10px] text-[#52525B]">Start minimized on launch</p>
                  </div>
                  <Switch checked={minimizeToTray} onCheckedChange={handleToggleMinimizeToTray} />
                </div>
              </div>
            </section>

            {/* Keybinds Section */}
            <section>
              <h3 className="text-[10px] font-semibold uppercase tracking-wider text-[#52525B] mb-3">
                Keybinds
              </h3>
              <div className="space-y-1">
                <Label className="text-xs text-[#A1A1AA]">Toggle Overlay</Label>
                <KeybindRecorder value={toggleKeybind} onChange={handleKeybindChange} />
              </div>
            </section>

            {/* Scanner Section */}
            <section>
              <h3 className="text-[10px] font-semibold uppercase tracking-wider text-[#52525B] mb-3">
                Scanner
              </h3>
              <div className="space-y-2">
                <button
                  onClick={() => setExcludeOpen(true)}
                  className="flex w-full items-center justify-between rounded-md border border-[#27272A] bg-[#27272A]/50 px-3 py-2 text-xs text-[#A1A1AA] hover:bg-[#27272A] hover:text-[#FAFAFA] transition-colors"
                >
                  <span>Exclude List</span>
                  <span className="flex items-center gap-1 text-[10px] text-[#52525B]">
                    {excludeList.length} items
                    <ChevronRight className="h-3 w-3" />
                  </span>
                </button>
                <button
                  onClick={() => setIncludeOpen(true)}
                  className="flex w-full items-center justify-between rounded-md border border-[#27272A] bg-[#27272A]/50 px-3 py-2 text-xs text-[#A1A1AA] hover:bg-[#27272A] hover:text-[#FAFAFA] transition-colors"
                >
                  <span>Include List</span>
                  <span className="flex items-center gap-1 text-[10px] text-[#52525B]">
                    {includeList.length} items
                    <ChevronRight className="h-3 w-3" />
                  </span>
                </button>
                <button
                  onClick={() => setDirectoriesOpen(true)}
                  className="flex w-full items-center justify-between rounded-md border border-[#27272A] bg-[#27272A]/50 px-3 py-2 text-xs text-[#A1A1AA] hover:bg-[#27272A] hover:text-[#FAFAFA] transition-colors"
                >
                  <span>Game Directories</span>
                  <span className="flex items-center gap-1 text-[10px] text-[#52525B]">
                    {gameDirectories.length} items
                    <ChevronRight className="h-3 w-3" />
                  </span>
                </button>
              </div>
            </section>

            {/* Integrations Section */}
            <section>
              <h3 className="text-[10px] font-semibold uppercase tracking-wider text-[#52525B] mb-3">
                Integrations
              </h3>
              <div className="space-y-2">
                <button
                  onClick={() => state.hwinfoConnected && setHwinfoOpen(true)}
                  className="flex w-full items-center justify-between rounded-md border border-[#27272A] bg-[#27272A]/50 px-3 py-2 text-left hover:bg-[#27272A] transition-colors"
                >
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-[#FAFAFA]">HWiNFO</span>
                    {state.hwinfoConnected ? (
                      <span className="flex items-center gap-1 text-[10px] text-[#22C55E]">
                        <span className="h-1.5 w-1.5 rounded-full bg-[#22C55E]" />
                        Detected
                      </span>
                    ) : (
                      <span className="flex items-center gap-1 text-[10px] text-[#52525B]">
                        <span className="h-1.5 w-1.5 rounded-full bg-[#52525B]" />
                        Not detected
                      </span>
                    )}
                  </div>
                  {state.hwinfoConnected && (
                    <span className="flex items-center gap-1 text-[10px] text-[#52525B]">
                      {state.hwinfoSensorCount} sensors
                      <ChevronRight className="h-3 w-3" />
                    </span>
                  )}
                </button>
              </div>
            </section>

            {/* Service Section */}
            <section>
              <h3 className="text-[10px] font-semibold uppercase tracking-wider text-[#52525B] mb-3">
                Service
              </h3>
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <Label className="text-xs text-[#A1A1AA]">omni-host</Label>
                  <div className="flex items-center gap-1.5">
                    <span
                      className={`h-1.5 w-1.5 rounded-full ${
                        state.connected
                          ? 'bg-[#22C55E] shadow-[0_0_4px_#22C55E66]'
                          : 'bg-[#EF4444] shadow-[0_0_4px_#EF444466]'
                      }`}
                    />
                    <span
                      className={`text-[11px] ${
                        state.connected ? 'text-[#22C55E]' : 'text-[#EF4444]'
                      }`}
                    >
                      {state.connected ? 'Connected' : 'Disconnected'}
                    </span>
                  </div>
                </div>
                <div className="flex justify-between">
                  <Button
                    size="sm"
                    onClick={handleRestartHost}
                    disabled={restarting}
                    className="h-8 gap-1.5 border border-[#EF4444]/30 bg-[#EF4444]/10 text-[#EF4444] hover:bg-[#EF4444]/20 hover:text-[#EF4444] text-xs"
                  >
                    <RotateCw className={`h-3 w-3 ${restarting ? 'animate-spin' : ''}`} />
                    {restarting ? 'Restarting...' : 'Restart Service'}
                  </Button>
                  <Button
                    size="sm"
                    onClick={() => router.push('/logs')}
                    className="h-8 gap-1.5 border border-[#00D9FF]/30 bg-[#00D9FF]/10 text-[#00D9FF] hover:bg-[#00D9FF]/20 hover:text-[#00D9FF] text-xs"
                  >
                    <ScrollText className="h-3 w-3" />
                    View Logs
                  </Button>
                </div>
              </div>
            </section>

            {/* Version */}
            <section className="pt-2 border-t border-[#27272A]">
              <span className="text-[10px] text-[#52525B] font-mono">
                <span className="text-[9px] text-[#52525B] font-mono block">VERSION</span>
                {process.env.OMNI_VERSION}
              </span>
            </section>
          </div>
        </ScrollArea>
      </div>

      <ProcessListDialog
        open={excludeOpen}
        onOpenChange={setExcludeOpen}
        title="Exclude List"
        processes={excludeList}
        onUpdate={(processes) => updateConfig({ exclude: processes })}
      />
      <ProcessListDialog
        open={includeOpen}
        onOpenChange={setIncludeOpen}
        title="Include List"
        processes={includeList}
        onUpdate={(processes) => updateConfig({ include: processes })}
      />
      <GameDirectoriesDialog
        open={directoriesOpen}
        onOpenChange={setDirectoriesOpen}
        directories={gameDirectories}
        onUpdate={(directories) => updateConfig({ game_directories: directories })}
      />
      <HwInfoSensorsDialog open={hwinfoOpen} onOpenChange={setHwinfoOpen} />
    </>
  );
}

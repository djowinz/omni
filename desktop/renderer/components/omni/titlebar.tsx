import { Minus, Square, X } from 'lucide-react';

declare global {
  interface Window {
    omni?: {
      onHostStatus: (callback: (status: any) => void) => void;
      minimizeWindow: () => void;
      maximizeWindow: () => void;
      closeWindow: () => void;
    };
  }
}

export function Titlebar() {
  return (
    <div
      className="flex h-8 items-center justify-between bg-[#0D0D0F] border-b border-[#27272A] select-none"
      style={{ WebkitAppRegion: 'drag' } as React.CSSProperties}
    >
      {/* Left: app identity */}
      <div className="flex items-center gap-2 pl-3">
        <div className="w-4 h-4 rounded-sm bg-gradient-to-br from-[#00D9FF] to-[#A855F7]" />
        <span className="text-[11px] font-medium text-[#71717A]">Omni Overlay</span>
      </div>

      {/* Right: window controls */}
      <div
        className="flex h-full"
        style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
      >
        <button
          onClick={() => window.omni?.minimizeWindow()}
          className="flex items-center justify-center w-11 h-full text-[#71717A] hover:bg-[#27272A] hover:text-[#FAFAFA] transition-colors"
        >
          <Minus className="h-3.5 w-3.5" />
        </button>
        <button
          onClick={() => window.omni?.maximizeWindow()}
          className="flex items-center justify-center w-11 h-full text-[#71717A] hover:bg-[#27272A] hover:text-[#FAFAFA] transition-colors"
        >
          <Square className="h-3 w-3" />
        </button>
        <button
          onClick={() => window.omni?.closeWindow()}
          className="flex items-center justify-center w-11 h-full text-[#71717A] hover:bg-[#EF4444] hover:text-[#FAFAFA] transition-colors"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}

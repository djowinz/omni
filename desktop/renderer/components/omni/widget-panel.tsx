import { useMemo } from "react";
import { Switch } from "@/components/ui/switch";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useOmniState } from "@/hooks/use-omni-state";
import {
  parseOmniContent,
  toggleWidgetEnabled,
  parseThemeImports,
} from "@/lib/omni-parser";
import { cn } from "@/lib/utils";
import { Layers, Eye, EyeOff, Palette, FileCode, Puzzle } from "lucide-react";

export function WidgetPanel() {
  const { state, dispatch, getCurrentOverlay, openThemeTab } = useOmniState();
  const currentOverlay = getCurrentOverlay();

  const widgets = useMemo(
    () => (currentOverlay?.content ? parseOmniContent(currentOverlay.content) : []),
    [currentOverlay?.content],
  );
  const themes = useMemo(
    () => (currentOverlay?.content ? parseThemeImports(currentOverlay.content) : []),
    [currentOverlay?.content],
  );

  const handleToggleWidget = (widgetId: string, enabled: boolean) => {
    if (!currentOverlay || currentOverlay.content === null) return;

    const newContent = toggleWidgetEnabled(
      currentOverlay.content,
      widgetId,
      enabled,
    );
    dispatch({
      type: "UPDATE_OVERLAY_CONTENT",
      payload: { name: currentOverlay.name, content: newContent },
    });
  };

  const handleSelectWidget = (widgetId: string) => {
    dispatch({ type: "SELECT_WIDGET", payload: widgetId });
  };

  const handleThemeClick = (themeSrc: string) => {
    openThemeTab(themeSrc);
  };

  const enabledCount = widgets.filter((w) => w.enabled).length;

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      {/* Panel Header */}
      <div className="flex h-10 items-center justify-between border-b border-[#27272A] px-3 bg-[#18181B]">
        <div className="flex items-center gap-2">
          <Layers className="h-4 w-4 text-[#A855F7]" />
          <h2 className="text-sm font-medium text-[#FAFAFA]">Components</h2>
        </div>
      </div>

      <ScrollArea className="flex-1">
        <div className="p-2">
          {/* Themes Section */}
          {themes.length > 0 && (
            <div className="mb-4">
              <div className="flex items-center gap-2 px-2 py-1.5 mb-1">
                <Palette className="h-3.5 w-3.5 text-[#00D9FF]" />
                <span className="text-xs font-medium text-[#71717A] uppercase tracking-wider">
                  Themes
                </span>
              </div>
              <div className="flex flex-col gap-1">
                {themes.map((theme) => (
                  <button
                    key={theme.src}
                    onClick={() => handleThemeClick(theme.src)}
                    className={cn(
                      "flex items-center gap-3 rounded-lg px-3 py-2 text-left transition-all",
                      "hover:bg-[#18181B] group focus:outline-none focus:ring-1 focus:ring-[#00D9FF]/50",
                    )}
                  >
                    <div className="w-8 h-8 rounded flex items-center justify-center bg-[#00D9FF]/10 text-[#00D9FF]">
                      <FileCode className="h-4 w-4" />
                    </div>
                    <div className="flex flex-col">
                      <span className="text-sm font-medium text-[#FAFAFA]">
                        {theme.name}
                      </span>
                      <span className="text-[10px] text-[#52525B] font-mono">
                        {theme.src}
                      </span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Widgets Section */}
          <div>
            <div className="flex items-center justify-between px-2 py-1.5 mb-1">
              <div className="flex items-center gap-2">
                <Puzzle className="h-3.5 w-3.5 text-[#A855F7]" />
                <span className="text-xs font-medium text-[#71717A] uppercase tracking-wider">
                  Widgets
                </span>
              </div>
              <span className="text-[10px] text-[#52525B]">
                {enabledCount}/{widgets.length}
              </span>
            </div>

            {widgets.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-8 text-center">
                <div className="w-12 h-12 rounded-lg bg-[#27272A] flex items-center justify-center mb-3">
                  <Layers className="h-6 w-6 text-[#71717A]" />
                </div>
                <p className="text-sm text-[#71717A]">No widgets found</p>
                <p className="text-xs text-[#52525B] mt-1">
                  Add a widget in the editor
                </p>
              </div>
            ) : (
              <div
                className="flex flex-col gap-1"
                role="listbox"
                aria-label="Widget list"
              >
                {widgets.map((widget) => (
                  <div
                    key={widget.id}
                    role="option"
                    aria-selected={state.selectedWidgetId === widget.id}
                    tabIndex={0}
                    onClick={() => handleSelectWidget(widget.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        handleSelectWidget(widget.id);
                      }
                    }}
                    className={cn(
                      "flex items-center justify-between rounded-lg px-3 py-2.5 text-left transition-all cursor-pointer",
                      "hover:bg-[#18181B] group focus:outline-none focus:ring-1 focus:ring-[#00D9FF]/50",
                      state.selectedWidgetId === widget.id &&
                        "bg-[#18181B] ring-1 ring-[#00D9FF]/50",
                    )}
                  >
                    <div className="flex items-center gap-3">
                      <div
                        className={cn(
                          "w-8 h-8 rounded flex items-center justify-center transition-colors",
                          widget.enabled
                            ? "bg-[#A855F7]/10 text-[#A855F7]"
                            : "bg-[#27272A] text-[#52525B]",
                        )}
                      >
                        {widget.enabled ? (
                          <Eye className="h-4 w-4" />
                        ) : (
                          <EyeOff className="h-4 w-4" />
                        )}
                      </div>
                      <div className="flex flex-col">
                        <span
                          className={cn(
                            "text-sm font-medium transition-colors",
                            widget.enabled
                              ? "text-[#FAFAFA]"
                              : "text-[#71717A]",
                          )}
                        >
                          {widget.name}
                        </span>
                        <span className="text-[10px] text-[#52525B] font-mono">
                          Line {widget.startLine}-{widget.endLine}
                        </span>
                      </div>
                    </div>
                    <Switch
                      checked={widget.enabled}
                      onCheckedChange={(checked) =>
                        handleToggleWidget(widget.id, checked)
                      }
                      onClick={(e) => e.stopPropagation()}
                      aria-label={`Toggle ${widget.name}`}
                      className="data-[state=checked]:bg-[#A855F7]"
                    />
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </ScrollArea>
    </div>
  );
}

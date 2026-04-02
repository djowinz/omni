import { useMemo, useState } from "react";
import { Switch } from "@/components/ui/switch";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useOmniState } from "@/hooks/use-omni-state";
import { useBackend } from "@/hooks/use-backend";
import {
  parseOmniContent,
  toggleWidgetEnabled,
  parseThemeImports,
} from "@/lib/omni-parser";
import { cn } from "@/lib/utils";
import { Layers, Eye, EyeOff, Palette, FileCode, Puzzle, Plus } from "lucide-react";

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

  const backend = useBackend();
  const [createThemeOpen, setCreateThemeOpen] = useState(false);
  const [browseThemesOpen, setBrowseThemesOpen] = useState(false);
  const [newThemeName, setNewThemeName] = useState('');
  const [availableThemes, setAvailableThemes] = useState<string[]>([]);

  /** Insert a <theme src="..."> tag at the top of the current overlay content. */
  const addThemeToOverlay = (themePath: string) => {
    if (!currentOverlay || currentOverlay.content === null) return;

    // Check if this theme is already imported
    const existingThemes = parseThemeImports(currentOverlay.content);
    if (existingThemes.some(t => t.src === themePath)) return;

    // Insert <theme src="..." /> at the top of the file (before first line or after existing theme tags)
    const themeTag = `<theme src="${themePath}" />\n`;
    const lines = currentOverlay.content.split('\n');

    // Find the last existing <theme> line to insert after, or insert at line 0
    let insertIndex = 0;
    for (let i = 0; i < lines.length; i++) {
      if (/<theme\s+/.test(lines[i])) {
        insertIndex = i + 1;
      }
    }

    lines.splice(insertIndex, 0, themeTag.trimEnd());
    const newContent = lines.join('\n');

    dispatch({
      type: 'UPDATE_OVERLAY_CONTENT',
      payload: { name: currentOverlay.name, content: newContent },
    });
    dispatch({ type: 'SET_DIRTY', payload: true });
  };

  const handleCreateTheme = async () => {
    const name = newThemeName.trim();
    if (!name) return;
    const filename = name.endsWith('.css') ? name : `${name}.css`;
    try {
      await backend.createTheme(filename);
      setCreateThemeOpen(false);
      setNewThemeName('');
      // Add to current overlay and open in editor
      addThemeToOverlay(`themes/${filename}`);
      openThemeTab(`themes/${filename}`);
    } catch (e) {
      console.error('Failed to create theme:', e);
    }
  };

  const handleBrowseThemes = async () => {
    try {
      const res = await backend.listFiles();
      setAvailableThemes(res.themes ?? []);
      setBrowseThemesOpen(true);
    } catch (e) {
      console.error('Failed to list themes:', e);
    }
  };

  const handleAddExistingTheme = (themeFilename: string) => {
    const themePath = `themes/${themeFilename}`;
    addThemeToOverlay(themePath);
    setBrowseThemesOpen(false);
    openThemeTab(themePath);
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
          {/* Themes Section — always visible */}
          <div className="mb-4">
            <div className="flex items-center justify-between px-2 py-1.5 mb-1">
              <div className="flex items-center gap-2">
                <Palette className="h-3.5 w-3.5 text-[#00D9FF]" />
                <span className="text-xs font-medium text-[#71717A] uppercase tracking-wider">
                  Themes
                </span>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={handleBrowseThemes}
                  className="text-[10px] text-[#71717A] hover:text-[#FAFAFA] transition-colors"
                  title="Add existing theme"
                >
                  Browse
                </button>
                <button
                  onClick={() => setCreateThemeOpen(true)}
                  className="flex items-center gap-1 text-[10px] text-[#00D9FF] hover:text-[#00D9FF]/80 transition-colors"
                  title="Create new theme"
                >
                  <Plus className="h-3 w-3" />
                  New
                </button>
              </div>
            </div>
            {themes.length > 0 ? (
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
            ) : (
              <div className="px-3 py-3 text-center">
                <p className="text-xs text-[#52525B]">No themes imported</p>
                <p className="text-[10px] text-[#3f3f46] mt-1">
                  Add <span className="font-mono text-[#00D9FF]/60">&lt;theme src="..." /&gt;</span> in your .omni file
                </p>
              </div>
            )}
          </div>

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

      {/* Create Theme Dialog */}
      <Dialog open={createThemeOpen} onOpenChange={setCreateThemeOpen}>
        <DialogContent className="bg-[#18181B] border-[#27272A]">
          <DialogHeader>
            <DialogTitle className="text-[#FAFAFA]">Create Theme</DialogTitle>
            <DialogDescription className="text-[#71717A]">
              Create a new CSS theme file. Reference it in your .omni file with{' '}
              <code className="text-[#00D9FF] font-mono text-xs">&lt;theme src="themes/name.css" /&gt;</code>
            </DialogDescription>
          </DialogHeader>
          <Input
            value={newThemeName}
            onChange={(e) => setNewThemeName(e.target.value)}
            placeholder="my-theme.css"
            className="bg-[#0D0D0F] border-[#27272A] text-[#FAFAFA] placeholder:text-[#52525B]"
            onKeyDown={(e) => { if (e.key === 'Enter') handleCreateTheme(); }}
          />
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setCreateThemeOpen(false)}
              className="text-[#71717A] hover:text-[#FAFAFA] hover:bg-[#27272A]"
            >
              Cancel
            </Button>
            <Button
              onClick={handleCreateTheme}
              disabled={!newThemeName.trim()}
              className="bg-[#00D9FF] text-[#0D0D0F] hover:bg-[#00D9FF]/90"
            >
              Create
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Browse Themes Dialog */}
      <Dialog open={browseThemesOpen} onOpenChange={setBrowseThemesOpen}>
        <DialogContent className="bg-[#18181B] border-[#27272A]">
          <DialogHeader>
            <DialogTitle className="text-[#FAFAFA]">Add Theme</DialogTitle>
            <DialogDescription className="text-[#71717A]">
              Select an existing theme to add to the current overlay.
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-1 max-h-[300px] overflow-y-auto">
            {availableThemes.length === 0 ? (
              <p className="text-sm text-[#52525B] text-center py-4">
                No themes available. Create one first.
              </p>
            ) : (
              availableThemes
                .filter(t => !themes.some(imported => imported.src === `themes/${t}`))
                .map((themeFile) => (
                  <button
                    key={themeFile}
                    onClick={() => handleAddExistingTheme(themeFile)}
                    className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-[#27272A] transition-colors"
                  >
                    <div className="w-8 h-8 rounded flex items-center justify-center bg-[#00D9FF]/10 text-[#00D9FF]">
                      <FileCode className="h-4 w-4" />
                    </div>
                    <div className="flex flex-col">
                      <span className="text-sm font-medium text-[#FAFAFA]">
                        {themeFile.replace(/\.css$/i, '')}
                      </span>
                      <span className="text-[10px] text-[#52525B] font-mono">
                        themes/{themeFile}
                      </span>
                    </div>
                  </button>
                ))
            )}
            {availableThemes.length > 0 &&
              availableThemes.filter(t => !themes.some(imported => imported.src === `themes/${t}`)).length === 0 && (
              <p className="text-sm text-[#52525B] text-center py-4">
                All available themes are already imported.
              </p>
            )}
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

import { useEffect, useCallback, useState, useRef } from 'react';
import Editor, { OnMount, BeforeMount } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';
import { Button } from '@/components/ui/button';
import { Code, Save, RotateCcw, X, Palette } from 'lucide-react';
import { useOmniState } from '@/hooks/use-omni-state';
import { useBackend } from '@/hooks/use-backend';
import { parseOmniContent } from '@/lib/omni-parser';
import { cn } from '@/lib/utils';
import { omniDarkTheme, registerOmniLanguage } from '@/lib/monaco-omni';
import type { ParseError } from '@/src/generated/ParseError';

export function EditorPanel() {
  const { state, dispatch, getCurrentOverlay, saveCurrentOverlay, closeTab, getActiveTab } = useOmniState();
  const currentOverlay = getCurrentOverlay();
  const activeTab = getActiveTab();

  const isShowingTab = activeTab != null;
  const displayContent = isShowingTab ? activeTab?.content : (currentOverlay?.content ?? '');
  const displayName = isShowingTab
    ? activeTab?.name
    : currentOverlay ? `${currentOverlay.name}.omni` : '';
  const displayType = isShowingTab ? activeTab?.type : 'overlay';

  // DEBUG: trace content flow
  console.log('[editor-panel]', {
    overlayName: currentOverlay?.name,
    contentLength: currentOverlay?.content?.length ?? 'NULL',
    displayContentLength: displayContent?.length ?? 'NULL',
    isShowingTab,
    activeTabId: activeTab?.id,
  });

  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<typeof import('monaco-editor') | null>(null);
  const [lineCount, setLineCount] = useState(1);

  // Determine Monaco language based on file type
  const language = displayType === 'theme' ? 'css' : 'omni';

  // Register theme and language before Monaco mounts
  const handleBeforeMount: BeforeMount = useCallback((monaco) => {
    monaco.editor.defineTheme('omni-dark', omniDarkTheme);
    registerOmniLanguage(monaco);
    monacoRef.current = monaco;
  }, []);

  // Pending scroll target — set when widget is clicked while on a different tab
  const pendingScrollRef = useRef<number | null>(null);

  // Capture editor reference on mount, execute pending scroll if any
  const handleMount: OnMount = useCallback((editor) => {
    editorRef.current = editor;
    setLineCount(editor.getModel()?.getLineCount() ?? 1);

    // Execute pending scroll from widget click
    if (pendingScrollRef.current !== null) {
      const line = pendingScrollRef.current;
      pendingScrollRef.current = null;
      editor.revealLineInCenter(line);
      editor.setPosition({ lineNumber: line, column: 1 });
      editor.focus();
    }
  }, []);

  // Handle content changes from Monaco
  const handleChange = useCallback((value: string | undefined) => {
    const code = value ?? '';
    setLineCount(code.split('\n').length);

    if (isShowingTab && activeTab) {
      dispatch({
        type: 'UPDATE_TAB_CONTENT',
        payload: { id: activeTab.id, content: code },
      });
      if (activeTab.type === 'overlay') {
        const overlayName = activeTab.id.replace('overlay:', '');
        dispatch({
          type: 'UPDATE_OVERLAY_CONTENT',
          payload: { name: overlayName, content: code },
        });
      }
    } else if (currentOverlay) {
      dispatch({
        type: 'UPDATE_OVERLAY_CONTENT',
        payload: { name: currentOverlay.name, content: code },
      });
      dispatch({ type: 'SET_DIRTY', payload: true });
    }
  }, [isShowingTab, activeTab, currentOverlay, dispatch]);

  const backend = useBackend();

  // Debounced parse: send content to backend every 400ms and set Monaco markers
  useEffect(() => {
    const content = displayContent ?? '';
    if (!content || !editorRef.current || !monacoRef.current) return;

    const timer = setTimeout(async () => {
      try {
        const { diagnostics } = await backend.parseOverlay(content);
        const model = editorRef.current?.getModel();
        if (model && monacoRef.current) {
          const markers = diagnostics.map((d: ParseError) => ({
            startLineNumber: d.line,
            startColumn: d.column,
            endLineNumber: d.line,
            endColumn: d.column + 10,
            severity: d.severity === 'Error'
              ? monacoRef.current!.MarkerSeverity.Error
              : monacoRef.current!.MarkerSeverity.Warning,
            message: d.message + (d.suggestion ? ` — ${d.suggestion}` : ''),
          }));
          monacoRef.current.editor.setModelMarkers(model, 'omni', markers);
        }
      } catch {
        // Host not connected — clear markers silently
        const model = editorRef.current?.getModel();
        if (model && monacoRef.current) {
          monacoRef.current.editor.setModelMarkers(model, 'omni', []);
        }
      }
    }, 400);

    return () => clearTimeout(timer);
  }, [displayContent, backend]);

  // Handle save — delegates to state hook which handles file write + auto-apply
  const handleSave = useCallback(async () => {
    await saveCurrentOverlay();
  }, [saveCurrentOverlay]);

  // Handle revert — reload content from backend
  const handleRevert = useCallback(async () => {
    if (!currentOverlay) return;
    try {
      const content = await backend.readFile(`overlays/${currentOverlay.name}/overlay.omni`);
      dispatch({ type: 'UPDATE_OVERLAY_CONTENT', payload: { name: currentOverlay.name, content } });
      dispatch({ type: 'SET_DIRTY', payload: false });
    } catch (e) {
      console.error('Revert failed:', e);
    }
  }, [currentOverlay, backend, dispatch]);

  // Keyboard shortcuts (Ctrl+S)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        handleSave();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleSave]);

  // Scroll to selected widget in editor when widget is clicked.
  // Watches widgetScrollRequest (increments on every click, even same widget).
  // Tab switching is handled by the widget panel — this only handles scrolling.
  useEffect(() => {
    if (!state.selectedWidgetId || !state.widgetScrollRequest || !currentOverlay?.content) return;
    // Don't try to scroll while a theme tab is showing — wait for tab to switch
    if (isShowingTab) {
      // Store pending scroll for when Monaco remounts with overlay content
      const widgets = parseOmniContent(currentOverlay.content);
      const widget = widgets.find(w => w.id === state.selectedWidgetId);
      if (widget) {
        pendingScrollRef.current = widget.startLine + 1;
      }
      return;
    }

    if (!editorRef.current) return;

    const widgets = parseOmniContent(currentOverlay.content);
    const widget = widgets.find(w => w.id === state.selectedWidgetId);
    if (!widget) return;

    const targetLine = widget.startLine + 1;
    editorRef.current.revealLineInCenter(targetLine);
    editorRef.current.setPosition({ lineNumber: targetLine, column: 1 });
    editorRef.current.focus();
  }, [state.widgetScrollRequest, isShowingTab]);

  // Handle closing a tab
  const handleCloseTab = (tabId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    closeTab(tabId);
  };

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      {/* Tab bar */}
      <div className="flex h-10 items-center border-b border-[#27272A] bg-[#18181B] overflow-x-auto">
        {/* Main overlay tab */}
        <button
          onClick={() => { if (isShowingTab) { dispatch({ type: 'SELECT_WIDGET', payload: null }); dispatch({ type: 'SET_ACTIVE_TAB', payload: null }); } }}
          className={cn(
            'flex items-center gap-2 px-4 h-full text-xs border-r border-[#27272A] transition-colors whitespace-nowrap',
            !isShowingTab
              ? 'bg-[#0D0D0F] text-[#FAFAFA] border-b-2 border-b-[#A855F7]'
              : 'text-[#71717A] hover:text-[#FAFAFA] hover:bg-[#27272A]/50'
          )}
        >
          <Code className="h-3.5 w-3.5 text-[#A855F7]" />
          {currentOverlay ? `${currentOverlay.name}.omni` : 'No overlay'}
          {state.isDirty && !isShowingTab && (
            <span className="w-2 h-2 rounded-full bg-[#F59E0B]" />
          )}
        </button>

        {/* Theme/file tabs */}
        {state.openTabs.map(tab => (
          <button
            key={tab.id}
            onClick={() => { if (activeTab?.id !== tab.id) { dispatch({ type: 'SELECT_WIDGET', payload: null }); dispatch({ type: 'SET_ACTIVE_TAB', payload: tab.id }); } }}
            className={cn(
              'group flex items-center gap-2 px-4 h-full text-xs border-r border-[#27272A] transition-colors whitespace-nowrap',
              activeTab?.id === tab.id
                ? 'bg-[#0D0D0F] text-[#FAFAFA] border-b-2 border-b-[#00D9FF]'
                : 'text-[#71717A] hover:text-[#FAFAFA] hover:bg-[#27272A]/50'
            )}
          >
            <Palette className="h-3.5 w-3.5 text-[#00D9FF]" />
            {tab.name}
            {tab.isDirty && (
              <span className="w-2 h-2 rounded-full bg-[#F59E0B]" />
            )}
            <span
              onClick={(e) => handleCloseTab(tab.id, e)}
              className="ml-1 p-0.5 rounded hover:bg-[#27272A] opacity-0 group-hover:opacity-100 transition-opacity"
            >
              <X className="h-3 w-3" />
            </span>
          </button>
        ))}

        {/* Spacer */}
        <div className="flex-1" />

        {/* Save/Revert buttons */}
        <div className="flex items-center gap-2 px-3">
          <span className="text-xs text-[#71717A] font-mono">{lineCount} lines</span>
          {state.isDirty && !isShowingTab && (
            <>
              <Button
                variant="ghost"
                size="sm"
                onClick={handleRevert}
                className="h-7 px-2 text-[#71717A] hover:text-[#FAFAFA] hover:bg-[#27272A]"
              >
                <RotateCcw className="h-3.5 w-3.5 mr-1" />
                Revert
              </Button>
              <Button
                size="sm"
                onClick={handleSave}
                className="h-7 px-2 bg-[#00D9FF] text-[#0D0D0F] hover:bg-[#00D9FF]/90"
              >
                <Save className="h-3.5 w-3.5 mr-1" />
                Save
              </Button>
            </>
          )}
        </div>
      </div>

      {/* Monaco editor */}
      <div className="flex-1 overflow-hidden">
        <Editor
          key={`${currentOverlay?.name ?? 'none'}-${activeTab?.id ?? 'main'}-${displayContent ? 'loaded' : 'empty'}`}
          theme="omni-dark"
          language={language}
          value={displayContent ?? ''}
          beforeMount={handleBeforeMount}
          onMount={handleMount}
          onChange={handleChange}
          options={{
            fontFamily: '"Monaspace Krypton", monospace',
            fontSize: 13,
            lineHeight: 20,
            minimap: { enabled: false },
            scrollBeyondLastLine: false,
            renderWhitespace: 'none',
            wordWrap: 'on',
            tabSize: 2,
            cursorBlinking: 'smooth',
            cursorSmoothCaretAnimation: 'on',
            smoothScrolling: true,
            padding: { top: 16 },
            overviewRulerLanes: 0,
            hideCursorInOverviewRuler: true,
            overviewRulerBorder: false,
            scrollbar: {
              verticalScrollbarSize: 8,
              horizontalScrollbarSize: 8,
            },
            lineNumbers: 'on',
            glyphMargin: false,
            folding: true,
            lineDecorationsWidth: 8,
            lineNumbersMinChars: 3,
            renderLineHighlight: 'line',
            contextmenu: true,
            quickSuggestions: false,
            automaticLayout: true,
          }}
        />
      </div>

      {/* Status bar */}
      <div className="flex h-6 items-center justify-between border-t border-[#27272A] bg-[#18181B] px-3">
        <div className="flex items-center gap-3 text-[10px] text-[#52525B]">
          <span className={displayType === 'theme' ? 'text-[#00D9FF]' : 'text-[#A855F7]'}>
            {displayType === 'theme' ? 'CSS' : 'OMNI'}
          </span>
          <span>UTF-8</span>
        </div>
        <div className="flex items-center gap-3 text-[10px] text-[#52525B]">
          <span className="flex items-center gap-1">
            <kbd className="px-1 py-0.5 bg-[#27272A] rounded text-[#71717A]">Ctrl+S</kbd>
            <span>Save</span>
          </span>
        </div>
      </div>
    </div>
  );
}

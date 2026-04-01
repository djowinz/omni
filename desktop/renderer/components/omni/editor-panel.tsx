

import { useEffect, useRef, useCallback, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Code, Save, RotateCcw, X, Palette } from 'lucide-react';
import { useOmniState } from '@/hooks/use-omni-state';
import { parseOmniContent } from '@/lib/omni-parser';
import { cn } from '@/lib/utils';

// Custom syntax highlighter for .omni files using Omni color palette
function highlightOmniSyntax(code: string): string {
  let result = code
    // Escape HTML first
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
  
  // Template variables like {cpu.usage} - yellow with background
  result = result.replace(
    /(\{[a-z][a-z0-9.]*\})/gi,
    '<span style="color: #F59E0B; background: rgba(245, 158, 11, 0.15); border-radius: 3px; padding: 1px 3px;">$1</span>'
  );
  
  // Class bindings like class:critical - pink
  result = result.replace(
    /(class:[a-z][a-z0-9-]*)/gi,
    '<span style="color: #EC4899;">$1</span>'
  );
  
  // Comments <!-- -->
  result = result.replace(
    /(&lt;!--[\s\S]*?--&gt;)/g,
    '<span style="color: #52525B; font-style: italic;">$1</span>'
  );
  
  // HTML/XML tags - cyan
  result = result.replace(
    /(&lt;\/?)([\w-]+)/g,
    '<span style="color: #71717A;">$1</span><span style="color: #00D9FF;">$2</span>'
  );
  
  // Closing bracket
  result = result.replace(
    /(\/?&gt;)/g,
    '<span style="color: #71717A;">$1</span>'
  );
  
  // Attribute names - purple
  result = result.replace(
    /\s([a-z][a-z0-9-]*)(=)/gi,
    ' <span style="color: #A855F7;">$1</span><span style="color: #71717A;">$2</span>'
  );
  
  // String values (double quotes) - green
  result = result.replace(
    /("(?:[^"\\]|\\.)*")/g,
    '<span style="color: #22C55E;">$1</span>'
  );
  
  // String values (single quotes) - green
  result = result.replace(
    /('(?:[^'\\]|\\.)*')/g,
    '<span style="color: #22C55E;">$1</span>'
  );
  
  // CSS property names (word followed by :) - blue
  result = result.replace(
    /([a-z-]+)(\s*:\s*)(?=[^;{]+[;}])/gi,
    '<span style="color: #3B82F6;">$1</span><span style="color: #71717A;">$2</span>'
  );
  
  // Numbers - orange
  result = result.replace(
    /\b(\d+(?:\.\d+)?)(px|%|em|rem|vh|vw|deg|s|ms)?\b/g,
    '<span style="color: #F97316;">$1</span><span style="color: #F59E0B;">$2</span>'
  );
  
  return result;
}

// Custom syntax highlighter for CSS files
function highlightCssSyntax(code: string): string {
  let result = code
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
  
  // Comments
  result = result.replace(
    /(\/\*[\s\S]*?\*\/)/g,
    '<span style="color: #52525B; font-style: italic;">$1</span>'
  );
  
  // Selectors - cyan
  result = result.replace(
    /^([.#]?[a-z][a-z0-9_-]*(?:\s*,\s*[.#]?[a-z][a-z0-9_-]*)*)(\s*\{)/gim,
    '<span style="color: #00D9FF;">$1</span><span style="color: #71717A;">$2</span>'
  );
  
  // Property names - blue
  result = result.replace(
    /([a-z-]+)(\s*:\s*)(?=[^;]+;)/gi,
    '<span style="color: #3B82F6;">$1</span><span style="color: #71717A;">$2</span>'
  );
  
  // Values in quotes - green
  result = result.replace(
    /("(?:[^"\\]|\\.)*")/g,
    '<span style="color: #22C55E;">$1</span>'
  );
  
  // Numbers - orange
  result = result.replace(
    /\b(\d+(?:\.\d+)?)(px|%|em|rem|vh|vw|deg|s|ms)?\b/g,
    '<span style="color: #F97316;">$1</span><span style="color: #F59E0B;">$2</span>'
  );
  
  // Hex colors - green
  result = result.replace(
    /(#[a-fA-F0-9]{3,8})\b/g,
    '<span style="color: #22C55E;">$1</span>'
  );
  
  // rgba/rgb functions - purple function, values
  result = result.replace(
    /\b(rgba?|hsla?|url|linear-gradient|radial-gradient)(\()/gi,
    '<span style="color: #A855F7;">$1</span><span style="color: #71717A;">$2</span>'
  );
  
  return result;
}

export function EditorPanel() {
  const { state, dispatch, getCurrentOverlay, saveCurrentOverlay, closeTab, getActiveTab } = useOmniState();
  const currentOverlay = getCurrentOverlay();
  const activeTab = getActiveTab();
  
  const isShowingTab = activeTab !== null;
  const displayContent = isShowingTab ? activeTab?.content : currentOverlay?.content;
  const displayName = isShowingTab 
    ? activeTab?.name 
    : currentOverlay ? `${currentOverlay.name}.omni` : '';
  const displayType = isShowingTab ? activeTab?.type : 'overlay';
  
  const [localContent, setLocalContent] = useState(displayContent || '');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const highlightRef = useRef<HTMLPreElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [lineCount, setLineCount] = useState(1);

  // Sync local content when display changes
  useEffect(() => {
    const content = displayContent || '';
    setLocalContent(content);
    setLineCount(content.split('\n').length);
  }, [displayContent, activeTab?.id, currentOverlay?.id]);

  // Handle content change
  const handleChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const code = e.target.value;
    setLocalContent(code);
    setLineCount(code.split('\n').length);
    
    if (isShowingTab && activeTab) {
      dispatch({
        type: 'UPDATE_TAB_CONTENT',
        payload: { id: activeTab.id, content: code },
      });
      if (activeTab.type === 'overlay') {
        const overlayId = activeTab.id.replace('overlay:', '');
        dispatch({
          type: 'UPDATE_OVERLAY_CONTENT',
          payload: { id: overlayId, content: code },
        });
      }
    } else if (currentOverlay) {
      dispatch({
        type: 'UPDATE_OVERLAY_CONTENT',
        payload: { id: currentOverlay.id, content: code },
      });
    }
  }, [isShowingTab, activeTab, currentOverlay, dispatch]);

  // Sync scroll between textarea and highlight
  const handleScroll = useCallback(() => {
    if (textareaRef.current && highlightRef.current) {
      highlightRef.current.scrollTop = textareaRef.current.scrollTop;
      highlightRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  }, []);

  // Handle save
  const handleSave = useCallback(async () => {
    if (currentOverlay && state.isDirty) {
      await saveCurrentOverlay();
    }
  }, [currentOverlay, state.isDirty, saveCurrentOverlay]);

  // Handle revert
  const handleRevert = useCallback(() => {
    setLocalContent(displayContent || '');
    dispatch({ type: 'SET_DIRTY', payload: false });
  }, [displayContent, dispatch]);

  // Keyboard shortcuts
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

  // Scroll to selected widget in editor
  useEffect(() => {
    if (!state.selectedWidgetId || !currentOverlay || !containerRef.current || isShowingTab) return;
    
    const widgets = parseOmniContent(currentOverlay.content);
    const widget = widgets.find(w => w.id === state.selectedWidgetId);
    
    if (widget && containerRef.current) {
      const lineHeight = 20;
      const scrollTop = (widget.startLine - 1) * lineHeight;
      containerRef.current.scrollTop = Math.max(0, scrollTop - 50);
    }
  }, [state.selectedWidgetId, currentOverlay, isShowingTab]);

  // Handle tab close
  const handleCloseTab = (tabId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    closeTab(tabId);
  };

  // Handle tab click
  const handleTabClick = (tabId: string) => {
    dispatch({ type: 'SET_ACTIVE_TAB', payload: tabId });
  };

  // Handle clicking the main overlay tab
  const handleMainTabClick = () => {
    dispatch({ type: 'SET_ACTIVE_TAB', payload: null });
  };

  // Highlighted code
  const highlightedCode = displayType === 'theme' 
    ? highlightCssSyntax(localContent)
    : highlightOmniSyntax(localContent);

  if (!currentOverlay && !activeTab) {
    return (
      <div className="flex h-full items-center justify-center bg-[#0D0D0F]">
        <p className="text-[#52525B]">No overlay selected</p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      {/* Tab Bar */}
      <div className="flex h-10 items-center border-b border-[#27272A] bg-[#18181B]">
        {/* Main overlay tab */}
        {currentOverlay && (
          <button
            onClick={handleMainTabClick}
            className={cn(
              'flex items-center gap-2 h-full px-4 text-sm border-r border-[#27272A] transition-colors',
              !isShowingTab 
                ? 'bg-[#0D0D0F] text-[#FAFAFA]' 
                : 'bg-[#18181B] text-[#71717A] hover:text-[#A1A1AA]'
            )}
          >
            <Code className="h-3.5 w-3.5 text-[#3B82F6]" />
            <span>{currentOverlay.name}</span>
            <span className="text-[#00D9FF]">.omni</span>
            {state.isDirty && !isShowingTab && (
              <span className="w-2 h-2 rounded-full bg-[#F59E0B]" />
            )}
          </button>
        )}
        
        {/* Theme tabs */}
        {state.openTabs.filter(t => t.type === 'theme').map(tab => (
          <button
            key={tab.id}
            onClick={() => handleTabClick(tab.id)}
            className={cn(
              'flex items-center gap-2 h-full px-4 text-sm border-r border-[#27272A] transition-colors group',
              state.activeTabId === tab.id 
                ? 'bg-[#0D0D0F] text-[#FAFAFA]' 
                : 'bg-[#18181B] text-[#71717A] hover:text-[#A1A1AA]'
            )}
          >
            <Palette className="h-3.5 w-3.5 text-[#00D9FF]" />
            <span>{tab.name}</span>
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

      {/* Editor with line numbers */}
      <div className="flex flex-1 overflow-hidden">
        {/* Line numbers */}
        <div 
          className="flex-shrink-0 select-none bg-[#0a0a0c] border-r border-[#27272A] pt-4 px-2 overflow-hidden"
          style={{ fontFamily: '"Monaspace Krypton", monospace', fontSize: 13, lineHeight: '20px' }}
        >
          {Array.from({ length: lineCount }, (_, i) => (
            <div key={i + 1} className="text-right text-[#52525B] pr-2" style={{ height: 20 }}>
              {i + 1}
            </div>
          ))}
        </div>
        
        {/* Editor area - layered textarea over highlighted pre */}
        <div 
          ref={containerRef}
          className="flex-1 relative overflow-auto"
        >
          {/* Highlighted code (background layer) */}
          <pre
            ref={highlightRef}
            className="absolute inset-0 p-4 m-0 overflow-hidden pointer-events-none whitespace-pre-wrap break-words"
            style={{
              fontFamily: '"Geist Mono", monospace',
              fontSize: 13,
              lineHeight: '20px',
              color: '#FAFAFA',
              backgroundColor: '#0D0D0F',
            }}
            dangerouslySetInnerHTML={{ __html: highlightedCode + '\n' }}
            aria-hidden="true"
          />
          
          {/* Textarea (foreground layer - transparent text) */}
          <textarea
            ref={textareaRef}
            value={localContent}
            onChange={handleChange}
            onScroll={handleScroll}
            spellCheck={false}
            className="absolute inset-0 w-full h-full p-4 m-0 resize-none border-0 outline-none bg-transparent"
            style={{
              fontFamily: '"Geist Mono", monospace',
              fontSize: 13,
              lineHeight: '20px',
              color: 'transparent',
              caretColor: '#00D9FF',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
            }}
          />
        </div>
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

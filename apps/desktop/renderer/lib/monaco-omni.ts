/**
 * Custom Monaco editor theme and language definitions for Omni.
 *
 * - "omni-dark" theme: comprehensive dark theme using the full Omni accent palette
 * - "omni" language: Monarch tokenizer for .omni files (XML with embedded CSS + {sensor.path})
 *
 * Color assignments:
 *   Cyan    #00D9FF  — XML/HTML element tags, CSS selectors
 *   Purple  #A855F7  — attribute names, CSS functions, keywords
 *   Green   #22C55E  — string values (attribute values, CSS strings, hex colors)
 *   Yellow  #F59E0B  — sensor interpolation variables {cpu.usage}
 *   Orange  #F97316  — numbers and numeric values
 *   Pink    #EC4899  — class bindings (class:name), special Omni attributes
 *   Blue    #3B82F6  — CSS property names
 *   Red     #EF4444  — !important, errors
 *   Gray    #71717A  — punctuation, delimiters, operators
 *   Muted   #52525B  — comments
 *   White   #FAFAFA  — default text, content
 */

import type { editor, languages } from 'monaco-editor';
import { getCSSLanguageService } from 'vscode-css-languageservice';
import { getLanguageService as getHTMLLanguageService } from 'vscode-html-languageservice';
import { TextDocument } from 'vscode-languageserver-textdocument';

export const omniDarkTheme: editor.IStandaloneThemeData = {
  base: 'vs-dark',
  inherit: false,
  rules: [
    // Base
    { token: '', foreground: 'FAFAFA', background: '0D0D0F' },

    // Comments
    { token: 'comment.xml', foreground: '52525B', fontStyle: 'italic' },
    { token: 'comment.css', foreground: '52525B', fontStyle: 'italic' },

    // XML/HTML element tags
    { token: 'tag', foreground: '00D9FF' },
    { token: 'tag.open', foreground: '71717A' },
    { token: 'tag.close', foreground: '71717A' },
    { token: 'tag.self-close', foreground: '71717A' },

    // XML/HTML attributes
    { token: 'attribute.name', foreground: 'A855F7' },
    { token: 'attribute.value', foreground: '22C55E' },
    { token: 'delimiter.equals', foreground: '71717A' },

    // Omni-specific
    { token: 'omni.variable', foreground: 'F59E0B', fontStyle: 'bold' },
    { token: 'omni.class-binding', foreground: 'EC4899' },
    { token: 'omni.class-binding-attr', foreground: 'EC4899' },

    // Text content
    { token: 'text.content', foreground: 'FAFAFA' },

    // CSS selectors
    { token: 'css.selector.class', foreground: '00D9FF' },
    { token: 'css.selector.id', foreground: '00D9FF', fontStyle: 'bold' },
    { token: 'css.selector.element', foreground: '00D9FF' },
    { token: 'css.selector.pseudo', foreground: '00D9FF', fontStyle: 'italic' },

    // CSS properties & values
    { token: 'css.property', foreground: '3B82F6' },
    { token: 'css.value.string', foreground: '22C55E' },
    { token: 'css.value.color', foreground: '22C55E' },
    { token: 'css.value.number', foreground: 'F97316' },
    { token: 'css.value.unit', foreground: 'F59E0B' },
    { token: 'css.value.keyword', foreground: 'A855F7' },
    { token: 'css.value.important', foreground: 'EF4444', fontStyle: 'bold' },

    // CSS functions & variables
    { token: 'css.function', foreground: 'A855F7' },
    { token: 'css.variable', foreground: 'F59E0B' },
    { token: 'css.custom-property', foreground: 'F59E0B' },

    // CSS punctuation
    { token: 'css.brace.open', foreground: '71717A' },
    { token: 'css.brace.close', foreground: '71717A' },
    { token: 'css.colon', foreground: '71717A' },
    { token: 'css.semicolon', foreground: '71717A' },
    { token: 'css.paren', foreground: '71717A' },
    { token: 'css.comma', foreground: '71717A' },
  ],
  colors: {
    'editor.background': '#0D0D0F',
    'editor.foreground': '#FAFAFA',
    'editor.lineHighlightBackground': '#18181B',
    'editor.lineHighlightBorder': '#00000000',
    'editorCursor.foreground': '#00D9FF',
    'editor.selectionBackground': '#00D9FF30',
    'editor.inactiveSelectionBackground': '#00D9FF15',
    'editor.selectionHighlightBackground': '#00D9FF15',
    'editorLineNumber.foreground': '#52525B',
    'editorLineNumber.activeForeground': '#A1A1AA',
    'editorGutter.background': '#0a0a0c',
    'editorGutter.addedBackground': '#22C55E',
    'editorGutter.modifiedBackground': '#3B82F6',
    'editorGutter.deletedBackground': '#EF4444',
    'editorIndentGuide.background': '#27272A',
    'editorIndentGuide.activeBackground': '#52525B',
    'editorBracketMatch.background': '#00D9FF20',
    'editorBracketMatch.border': '#00D9FF50',
    'editor.findMatchBackground': '#F59E0B40',
    'editor.findMatchHighlightBackground': '#F59E0B20',
    'editor.findMatchBorder': '#F59E0B',
    'editorWidget.background': '#18181B',
    'editorWidget.border': '#27272A',
    'editorWidget.foreground': '#FAFAFA',
    'editorSuggestWidget.background': '#18181B',
    'editorSuggestWidget.border': '#27272A',
    'editorSuggestWidget.foreground': '#FAFAFA',
    'editorSuggestWidget.selectedBackground': '#27272A',
    'editorSuggestWidget.highlightForeground': '#00D9FF',
    'editorHoverWidget.background': '#18181B',
    'editorHoverWidget.border': '#27272A',
    'input.background': '#0D0D0F',
    'input.border': '#27272A',
    'input.foreground': '#FAFAFA',
    'input.placeholderForeground': '#52525B',
    'inputOption.activeBorder': '#00D9FF',
    'scrollbar.shadow': '#00000000',
    'scrollbarSlider.background': '#27272A80',
    'scrollbarSlider.hoverBackground': '#52525B80',
    'scrollbarSlider.activeBackground': '#71717A80',
    'minimap.background': '#0D0D0F',
    'editorError.foreground': '#EF4444',
    'editorWarning.foreground': '#F59E0B',
    'editorInfo.foreground': '#3B82F6',
    'editorOverviewRuler.border': '#00000000',
    'editorOverviewRuler.errorForeground': '#EF4444',
    'editorOverviewRuler.warningForeground': '#F59E0B',
  },
};

// ── Context detection & block extraction ────────────────────────────

type OmniContext = 'css' | 'template' | 'other';

function getOmniContext(
  model: { getValueInRange: (range: any) => string },
  position: { lineNumber: number; column: number },
): OmniContext {
  const textBefore = model.getValueInRange({
    startLineNumber: 1,
    startColumn: 1,
    endLineNumber: position.lineNumber,
    endColumn: position.column,
  });

  // Find ALL relevant tag boundaries and determine which one is most recent.
  // The most recent unmatched opening tag determines the context.
  const tagRe = /<(\/?)(?:style|template)([\s>])/gi;
  let context: OmniContext = 'other';
  let m: RegExpExecArray | null;

  while ((m = tagRe.exec(textBefore))) {
    const isClose = m[1] === '/';
    const tag = textBefore.substring(m.index + 1 + (isClose ? 1 : 0));
    const isStyle = tag.startsWith('style');

    if (isClose) {
      // Closing tag — we're no longer inside that block
      context = 'other';
    } else if (isStyle) {
      // Verify the opening tag is complete (has a >)
      const rest = textBefore.substring(m.index);
      if (rest.match(/^<style[^>]*>/i)) {
        context = 'css';
      }
    } else {
      // template opening
      const rest = textBefore.substring(m.index);
      if (rest.match(/^<template[^>]*>/i)) {
        context = 'template';
      }
    }
  }

  return context;
}

interface ExtractedBlock {
  text: string;
  startLine: number; // 1-based line where block content begins
}

/**
 * Extract the <style> block containing the given cursor position.
 * Handles multiple <style> blocks across different widgets.
 */
function extractStyleBlock(
  model: { getValue: () => string },
  position: { lineNumber: number; column: number },
): ExtractedBlock {
  return extractBlock(model.getValue(), position, 'style');
}

/**
 * Extract the <template> block containing the given cursor position.
 * Handles multiple <template> blocks across different widgets.
 */
function extractTemplateBlock(
  model: { getValue: () => string },
  position: { lineNumber: number; column: number },
): ExtractedBlock {
  return extractBlock(model.getValue(), position, 'template');
}

function extractBlock(
  full: string,
  position: { lineNumber: number; column: number },
  tag: string,
): ExtractedBlock {
  // Convert cursor position to character offset
  const lines = full.split('\n');
  let cursorOffset = 0;
  for (let i = 0; i < position.lineNumber - 1 && i < lines.length; i++) {
    cursorOffset += lines[i].length + 1; // +1 for \n
  }
  cursorOffset += position.column - 1;

  // Find ALL open/close pairs and pick the one containing the cursor
  const openRe = new RegExp(`<${tag}[^>]*>`, 'gi');
  let m: RegExpExecArray | null;

  while ((m = openRe.exec(full))) {
    const contentStart = m.index + m[0].length;
    const closeRe = new RegExp(`</${tag}>`, 'i');
    const closeMatch = closeRe.exec(full.slice(contentStart));
    const contentEnd = closeMatch ? contentStart + closeMatch.index : full.length;

    if (cursorOffset >= contentStart && cursorOffset <= contentEnd) {
      const text = full.slice(contentStart, contentEnd);
      const startLine = full.slice(0, contentStart).split('\n').length;
      return { text, startLine };
    }
  }

  return { text: '', startLine: 1 };
}

// ── LSP → Monaco mapping ────────────────────────────────────────────

function mapLspToMonaco(
  monaco: typeof import('monaco-editor'),
  lspList: { items: any[]; isIncomplete?: boolean },
  position: { lineNumber: number; column: number },
  blockStartLine: number,
): languages.CompletionList {
  const kindMap: Record<number, number> = {
    1: monaco.languages.CompletionItemKind.Text,
    2: monaco.languages.CompletionItemKind.Method,
    3: monaco.languages.CompletionItemKind.Function,
    4: monaco.languages.CompletionItemKind.Constructor,
    5: monaco.languages.CompletionItemKind.Field,
    6: monaco.languages.CompletionItemKind.Variable,
    7: monaco.languages.CompletionItemKind.Class,
    8: monaco.languages.CompletionItemKind.Interface,
    9: monaco.languages.CompletionItemKind.Module,
    10: monaco.languages.CompletionItemKind.Property,
    11: monaco.languages.CompletionItemKind.Unit,
    12: monaco.languages.CompletionItemKind.Value,
    13: monaco.languages.CompletionItemKind.Enum,
    14: monaco.languages.CompletionItemKind.Keyword,
    15: monaco.languages.CompletionItemKind.Snippet,
    16: monaco.languages.CompletionItemKind.Color,
    17: monaco.languages.CompletionItemKind.File,
    18: monaco.languages.CompletionItemKind.Reference,
    19: monaco.languages.CompletionItemKind.Folder,
    20: monaco.languages.CompletionItemKind.EnumMember,
    21: monaco.languages.CompletionItemKind.Constant,
    22: monaco.languages.CompletionItemKind.Struct,
    23: monaco.languages.CompletionItemKind.Event,
    24: monaco.languages.CompletionItemKind.Operator,
    25: monaco.languages.CompletionItemKind.TypeParameter,
  };

  const defaultRange = {
    startLineNumber: position.lineNumber,
    startColumn: position.column,
    endLineNumber: position.lineNumber,
    endColumn: position.column,
  };

  const suggestions: languages.CompletionItem[] = lspList.items.map((item, i) => {
    let range = defaultRange;
    if (item.textEdit && 'range' in item.textEdit) {
      const r = item.textEdit.range;
      range = {
        startLineNumber: r.start.line + blockStartLine,
        startColumn: r.start.character + 1,
        endLineNumber: r.end.line + blockStartLine,
        endColumn: r.end.character + 1,
      };
    } else if (item.textEdit && 'replace' in item.textEdit) {
      const r = item.textEdit.replace;
      range = {
        startLineNumber: r.start.line + blockStartLine,
        startColumn: r.start.character + 1,
        endLineNumber: r.end.line + blockStartLine,
        endColumn: r.end.character + 1,
      };
    }

    let documentation: string | { value: string } | undefined;
    if (item.documentation) {
      if (typeof item.documentation === 'string') {
        documentation = item.documentation;
      } else if (item.documentation.value) {
        documentation = { value: item.documentation.value };
      }
    }

    return {
      label: typeof item.label === 'string' ? item.label : item.label.label,
      kind: kindMap[item.kind ?? 1] ?? monaco.languages.CompletionItemKind.Text,
      detail: item.detail,
      documentation,
      insertText:
        item.textEdit?.newText ??
        item.insertText ??
        (typeof item.label === 'string' ? item.label : item.label.label),
      range,
      sortText: item.sortText ?? String(i).padStart(5, '0'),
      filterText: item.filterText,
      insertTextRules:
        item.insertTextFormat === 2
          ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
          : undefined,
    };
  });

  return { suggestions, incomplete: lspList.isIncomplete ?? false };
}

/** Register the custom Omni language for .omni files */
export function registerOmniLanguage(monaco: typeof import('monaco-editor')) {
  monaco.languages.register({ id: 'omni' });

  monaco.languages.setMonarchTokensProvider('omni', {
    defaultToken: 'text.content',
    tokenPostfix: '',
    ignoreCase: false,

    tokenizer: {
      root: [
        // XML comments
        [/<!--/, 'comment.xml', '@xmlComment'],

        // Sensor interpolation
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni.variable'],

        // Class bindings class:name
        [/(class:)([a-zA-Z][a-zA-Z0-9-]*)/, ['omni.class-binding', 'omni.class-binding-attr']],

        // <style> tag — enters CSS mode
        [/(<)(style)(\s*>)/, ['tag.open', 'tag', { token: 'tag.close', next: '@cssBlock' }]],
        [/(<)(style)(\s)/, ['tag.open', 'tag', { token: '', next: '@styleTagAttrs' }]],

        // </style> closing (shouldn't hit this in root, but safety)
        [/(<)(\/)(style)(>)/, ['tag.open', 'tag.open', 'tag', 'tag.close']],

        // Any other opening/closing tag
        [
          /(<)(\/?)([a-zA-Z][a-zA-Z0-9-]*)/,
          ['tag.open', 'tag.open', { token: 'tag', next: '@tagAttrs' }],
        ],

        // Text content
        [/[^<{]+/, 'text.content'],
      ],

      // <style ...> attributes before entering CSS
      styleTagAttrs: [
        [/>/, { token: 'tag.close', next: '@cssBlock' }],
        [/(\/)(>)/, ['tag.self-close', { token: 'tag.close', next: '@pop' }]],
        [/[a-zA-Z_:-][a-zA-Z0-9_:.-]*(?=\s*=)/, 'attribute.name'],
        [/=/, 'delimiter.equals'],
        [/"[^"]*"/, 'attribute.value'],
        [/'[^']*'/, 'attribute.value'],
        [/\s+/, ''],
      ],

      // Generic tag attributes
      tagAttrs: [
        [/(\/)(>)/, ['tag.self-close', { token: 'tag.close', next: '@pop' }]],
        [/>/, { token: 'tag.close', next: '@pop' }],
        [/(class:)([a-zA-Z][a-zA-Z0-9-]*)/, ['omni.class-binding', 'omni.class-binding-attr']],
        [/[a-zA-Z_:-][a-zA-Z0-9_:.-]*(?=\s*=)/, 'attribute.name'],
        [/=/, 'delimiter.equals'],
        [/"/, { token: 'attribute.value', next: '@attrValueDQ' }],
        [/'/, { token: 'attribute.value', next: '@attrValueSQ' }],
        [/[a-zA-Z_:-][a-zA-Z0-9_:.-]*/, 'attribute.name'],
        [/\s+/, ''],
      ],

      // Double-quoted attribute value (with variable support)
      attrValueDQ: [
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni.variable'],
        [/[^"{}]+/, 'attribute.value'],
        [/"/, { token: 'attribute.value', next: '@pop' }],
      ],

      // Single-quoted attribute value
      attrValueSQ: [
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni.variable'],
        [/[^'{}]+/, 'attribute.value'],
        [/'/, { token: 'attribute.value', next: '@pop' }],
      ],

      xmlComment: [
        [/-->/, 'comment.xml', '@pop'],
        [/./, 'comment.xml'],
      ],

      // ── CSS states ─────────────────────────────────────────

      cssBlock: [
        [
          /(<)(\/)(style)(>)/,
          ['tag.open', 'tag.open', 'tag', { token: 'tag.close', next: '@pop' }],
        ],
        [/\/\*/, 'comment.css', '@cssComment'],
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni.variable'],
        [/:root\b/, 'css.selector.pseudo'],
        [/\.[a-zA-Z][a-zA-Z0-9_-]*/, 'css.selector.class'],
        [/#[a-zA-Z][a-zA-Z0-9_-]*/, 'css.selector.id'],
        [/[a-zA-Z][a-zA-Z0-9-]*(?=\s*[,{])/, 'css.selector.element'],
        [/:[a-zA-Z][a-zA-Z0-9-]*/, 'css.selector.pseudo'],
        [/\{/, 'css.brace.open', '@cssProperties'],
        [/\}/, 'css.brace.close'],
        [/,/, 'css.comma'],
        [/\s+/, ''],
      ],

      cssProperties: [
        [/\}/, { token: 'css.brace.close', next: '@pop' }],
        [
          /(<)(\/)(style)(>)/,
          ['tag.open', 'tag.open', 'tag', { token: 'tag.close', next: '@popall' }],
        ],
        [/\/\*/, 'comment.css', '@cssComment'],
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni.variable'],
        [/--[a-zA-Z][a-zA-Z0-9-]*(?=\s*:)/, 'css.custom-property'],
        [/[a-zA-Z-]+(?=\s*:)/, 'css.property'],
        [/:/, 'css.colon', '@cssValue'],
        [/;/, 'css.semicolon'],
        [/\s+/, ''],
      ],

      cssValue: [
        [/;/, { token: 'css.semicolon', next: '@pop' }],
        [/(?=\})/, '', '@pop'],
        [
          /(<)(\/)(style)(>)/,
          ['tag.open', 'tag.open', 'tag', { token: 'tag.close', next: '@popall' }],
        ],
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni.variable'],
        [/!important\b/, 'css.value.important'],
        [/(var)(\()/, ['css.function', 'css.paren'], '@cssVarArgs'],
        [
          /(rgba?|hsla?|linear-gradient|radial-gradient|url|calc|min|max|clamp)(\()/,
          ['css.function', 'css.paren'],
        ],
        [/--[a-zA-Z][a-zA-Z0-9-]*/, 'css.custom-property'],
        [/#[a-fA-F0-9]{3,8}\b/, 'css.value.color'],
        [
          /(\d+\.?\d*)(px|%|em|rem|vh|vw|vmin|vmax|deg|s|ms|fr)\b/,
          ['css.value.number', 'css.value.unit'],
        ],
        [/\d+\.?\d*/, 'css.value.number'],
        [/"[^"]*"/, 'css.value.string'],
        [/'[^']*'/, 'css.value.string'],
        [
          /\b(none|auto|inherit|initial|unset|flex|grid|block|inline|inline-block|column|row|wrap|nowrap|center|start|end|stretch|space-between|space-around|space-evenly|fixed|relative|absolute|sticky|static|bold|normal|italic|hidden|visible|scroll|solid|dashed|dotted|transparent|currentColor)\b/,
          'css.value.keyword',
        ],
        [/,/, 'css.comma'],
        [/[()]/, 'css.paren'],
        [/\s+/, ''],
        [/[^\s;}{(),"']+/, 'css.value.keyword'],
      ],

      cssVarArgs: [
        [/--[a-zA-Z][a-zA-Z0-9-]*/, 'css.variable'],
        [/,/, 'css.comma'],
        [/\)/, { token: 'css.paren', next: '@pop' }],
        [/\s+/, ''],
        [/./, ''],
      ],

      cssComment: [
        [/\*\//, 'comment.css', '@pop'],
        [/./, 'comment.css'],
      ],
    },
  });

  monaco.languages.setLanguageConfiguration('omni', {
    comments: {
      blockComment: ['<!--', '-->'],
    },
    brackets: [
      ['{', '}'],
      ['[', ']'],
      ['(', ')'],
    ],
    autoClosingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: '<!--', close: '-->' },
    ],
    surroundingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: '<', close: '>' },
    ],
    indentationRules: {
      increaseIndentPattern:
        /(<(?!\/|!--|area|base|br|col|hr|img|input|link|meta|param)[a-zA-Z][a-zA-Z0-9-]*\b[^/>]*>(?!.*<\/\1>)\s*$)|\{[^}]*$/,
      decreaseIndentPattern: /^\s*(<\/[a-zA-Z][a-zA-Z0-9-]*\s*>|\})/,
    },
    onEnterRules: [
      {
        beforeText: /<([a-zA-Z][a-zA-Z0-9-]*)\b[^/>]*>$/,
        afterText: /^<\/([a-zA-Z][a-zA-Z0-9-]*)\s*>$/,
        action: {
          indentAction: monaco.languages.IndentAction.IndentOutdent,
        },
      },
      {
        beforeText: /<([a-zA-Z][a-zA-Z0-9-]*)\b[^/>]*>$/,
        action: {
          indentAction: monaco.languages.IndentAction.Indent,
        },
      },
      {
        beforeText: /\{[^}]*$/,
        afterText: /^\s*\}/,
        action: {
          indentAction: monaco.languages.IndentAction.IndentOutdent,
        },
      },
      {
        beforeText: /\{[^}]*$/,
        action: {
          indentAction: monaco.languages.IndentAction.Indent,
        },
      },
    ],
  });

  // CSS & HTML language services for autocompletion
  const cssService = getCSSLanguageService();
  const htmlService = getHTMLLanguageService();

  // CSS completion provider — triggers inside <style> blocks
  monaco.languages.registerCompletionItemProvider('omni', {
    triggerCharacters: [':', ' ', ';', '.', '#', '-', '/', '(', '!'],
    provideCompletionItems(model, position) {
      if (getOmniContext(model, position) !== 'css') return { suggestions: [] };

      const block = extractStyleBlock(model, position);
      if (!block.text) return { suggestions: [] };

      const virtualDoc = TextDocument.create('inmemory://omni.css', 'css', 1, block.text);
      const stylesheet = cssService.parseStylesheet(virtualDoc);
      const virtualPos = {
        line: position.lineNumber - block.startLine,
        character: position.column - 1,
      };
      const completions = cssService.doComplete(virtualDoc, virtualPos, stylesheet);

      return mapLspToMonaco(monaco, completions, position, block.startLine);
    },
  });

  // HTML completion provider — triggers inside <template> blocks
  monaco.languages.registerCompletionItemProvider('omni', {
    triggerCharacters: ['<', ' ', '"', '=', '/', '.'],
    provideCompletionItems(model, position) {
      if (getOmniContext(model, position) !== 'template') return { suggestions: [] };

      const block = extractTemplateBlock(model, position);
      if (!block.text) return { suggestions: [] };

      const virtualDoc = TextDocument.create('inmemory://omni.html', 'html', 1, block.text);
      const htmlDoc = htmlService.parseHTMLDocument(virtualDoc);
      const virtualPos = {
        line: position.lineNumber - block.startLine,
        character: position.column - 1,
      };
      const completions = htmlService.doComplete(virtualDoc, virtualPos, htmlDoc);

      return mapLspToMonaco(monaco, completions, position, block.startLine);
    },
  });

  // Sensor path autocomplete — triggers inside {…} placeholders
  const sensorItems: Array<{ path: string; detail: string; category: string }> = [
    { path: 'cpu.usage', detail: 'CPU usage %', category: 'CPU' },
    { path: 'cpu.temp', detail: 'CPU package temperature', category: 'CPU' },
    { path: 'gpu.usage', detail: 'GPU usage %', category: 'GPU' },
    { path: 'gpu.temp', detail: 'GPU temperature', category: 'GPU' },
    { path: 'gpu.clock', detail: 'GPU core clock (MHz)', category: 'GPU' },
    { path: 'gpu.mem-clock', detail: 'GPU memory clock (MHz)', category: 'GPU' },
    { path: 'gpu.vram', detail: 'VRAM used/total (e.g. 4096/12288)', category: 'GPU' },
    { path: 'gpu.vram.used', detail: 'VRAM used (MB)', category: 'GPU' },
    { path: 'gpu.vram.total', detail: 'VRAM total (MB)', category: 'GPU' },
    { path: 'gpu.power', detail: 'GPU power draw (W)', category: 'GPU' },
    { path: 'gpu.fan', detail: 'GPU fan speed %', category: 'GPU' },
    { path: 'ram.usage', detail: 'RAM usage %', category: 'RAM' },
    { path: 'ram.used', detail: 'RAM used (MB)', category: 'RAM' },
    { path: 'ram.total', detail: 'RAM total (MB)', category: 'RAM' },
    { path: 'fps', detail: 'Frames per second', category: 'Frame' },
    { path: 'frame-time', detail: 'Frame time (ms)', category: 'Frame' },
    { path: 'frame-time.avg', detail: 'Average frame time (ms)', category: 'Frame' },
    { path: 'frame-time.1pct', detail: '1% low frame time (ms)', category: 'Frame' },
    { path: 'frame-time.01pct', detail: '0.1% low frame time (ms)', category: 'Frame' },
  ];

  monaco.languages.registerCompletionItemProvider('omni', {
    triggerCharacters: ['{', '.', '-'],
    provideCompletionItems(model, position): languages.CompletionList {
      const lineContent = model.getLineContent(position.lineNumber);
      const textBefore = lineContent.substring(0, position.column - 1);

      // Context 1: Inside {…} interpolation braces
      const lastOpen = textBefore.lastIndexOf('{');
      if (lastOpen >= 0) {
        const between = textBefore.substring(lastOpen + 1);
        if (!between.includes('}')) {
          const range = {
            startLineNumber: position.lineNumber,
            startColumn: lastOpen + 2,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          };
          return { suggestions: makeSensorSuggestions(range) };
        }
      }

      // Context 2: Inside class binding value — class:name="…cursor…"
      // Match: class:word="  with no closing " after the opening one
      const classBindingMatch = textBefore.match(/class:[a-zA-Z0-9_-]+=["']([^"']*)$/);
      if (classBindingMatch) {
        // Find the start of the current word (sensor path token)
        const valueText = classBindingMatch[1];
        // The sensor path is the last word-like token: letters, digits, dots, dashes
        const tokenMatch = valueText.match(/([a-zA-Z][a-zA-Z0-9._-]*)$/);
        const tokenStart = tokenMatch ? position.column - tokenMatch[1].length : position.column;

        const range = {
          startLineNumber: position.lineNumber,
          startColumn: tokenStart,
          endLineNumber: position.lineNumber,
          endColumn: position.column,
        };
        return { suggestions: makeSensorSuggestions(range) };
      }

      return { suggestions: [] };
    },
  });

  function makeSensorSuggestions(range: {
    startLineNumber: number;
    startColumn: number;
    endLineNumber: number;
    endColumn: number;
  }): languages.CompletionItem[] {
    return sensorItems.map((s, i) => ({
      label: s.path,
      kind: monaco.languages.CompletionItemKind.Variable,
      detail: s.detail,
      documentation: `${s.category} sensor — use as {${s.path}}`,
      insertText: s.path,
      range,
      sortText: String(i).padStart(3, '0'),
    }));
  }
}

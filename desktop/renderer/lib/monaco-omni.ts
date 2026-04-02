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

import type { editor } from 'monaco-editor';

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
        [/(<)(\/?)([a-zA-Z][a-zA-Z0-9-]*)/, ['tag.open', 'tag.open', { token: 'tag', next: '@tagAttrs' }]],

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
        [/(<)(\/)(style)(>)/, ['tag.open', 'tag.open', 'tag', { token: 'tag.close', next: '@pop' }]],
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
        [/(<)(\/)(style)(>)/, ['tag.open', 'tag.open', 'tag', { token: 'tag.close', next: '@popall' }]],
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
        [/(<)(\/)(style)(>)/, ['tag.open', 'tag.open', 'tag', { token: 'tag.close', next: '@popall' }]],
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni.variable'],
        [/!important\b/, 'css.value.important'],
        [/(var)(\()/, ['css.function', 'css.paren'], '@cssVarArgs'],
        [/(rgba?|hsla?|linear-gradient|radial-gradient|url|calc|min|max|clamp)(\()/, ['css.function', 'css.paren']],
        [/--[a-zA-Z][a-zA-Z0-9-]*/, 'css.custom-property'],
        [/#[a-fA-F0-9]{3,8}\b/, 'css.value.color'],
        [/(\d+\.?\d*)(px|%|em|rem|vh|vw|vmin|vmax|deg|s|ms|fr)\b/, ['css.value.number', 'css.value.unit']],
        [/\d+\.?\d*/, 'css.value.number'],
        [/"[^"]*"/, 'css.value.string'],
        [/'[^']*'/, 'css.value.string'],
        [/\b(none|auto|inherit|initial|unset|flex|grid|block|inline|inline-block|column|row|wrap|nowrap|center|start|end|stretch|space-between|space-around|space-evenly|fixed|relative|absolute|sticky|static|bold|normal|italic|hidden|visible|scroll|solid|dashed|dotted|transparent|currentColor)\b/, 'css.value.keyword'],
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
      increaseIndentPattern: /(<(?!\/|!--|area|base|br|col|hr|img|input|link|meta|param)[a-zA-Z][a-zA-Z0-9-]*\b[^/>]*>(?!.*<\/\1>)\s*$)|\{[^}]*$/,
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
}

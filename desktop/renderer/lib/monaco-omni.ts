/**
 * Custom Monaco editor theme and language definitions for Omni.
 *
 * - "omni-dark" theme: matches the app's dark color palette with Monaspace Krypton font
 * - "omni" language: syntax highlighting for .omni files (XML + {sensor.path} interpolation)
 */

import type { editor } from 'monaco-editor';

/** Omni dark theme matching the app's color palette */
export const omniDarkTheme: editor.IStandaloneThemeData = {
  base: 'vs-dark',
  inherit: true,
  rules: [
    // General
    { token: '', foreground: 'FAFAFA', background: '0D0D0F' },
    { token: 'comment', foreground: '52525B', fontStyle: 'italic' },

    // XML/HTML tags
    { token: 'tag', foreground: '00D9FF' },
    { token: 'tag.attribute.name', foreground: 'A855F7' },
    { token: 'tag.attribute.value', foreground: '22C55E' },
    { token: 'delimiter.html', foreground: '71717A' },
    { token: 'metatag', foreground: '71717A' },

    // Omni-specific: sensor interpolation {cpu.usage}
    { token: 'omni-variable', foreground: 'F59E0B', fontStyle: 'bold' },
    { token: 'omni-class-binding', foreground: 'EC4899' },

    // CSS tokens
    { token: 'selector', foreground: '00D9FF' },
    { token: 'attribute.name.css', foreground: '3B82F6' },
    { token: 'attribute.value.css', foreground: '22C55E' },
    { token: 'number', foreground: 'F97316' },
    { token: 'number.css', foreground: 'F97316' },
    { token: 'unit.css', foreground: 'F59E0B' },
    { token: 'keyword', foreground: 'A855F7' },
    { token: 'string', foreground: '22C55E' },
    { token: 'string.css', foreground: '22C55E' },
    { token: 'variable.css', foreground: 'F59E0B' },

    // CSS function names
    { token: 'function.css', foreground: 'A855F7' },

    // Punctuation
    { token: 'delimiter', foreground: '71717A' },
    { token: 'delimiter.bracket', foreground: '71717A' },
    { token: 'delimiter.curly', foreground: '71717A' },
  ],
  colors: {
    'editor.background': '#0D0D0F',
    'editor.foreground': '#FAFAFA',
    'editor.lineHighlightBackground': '#18181B',
    'editor.selectionBackground': '#00D9FF30',
    'editor.inactiveSelectionBackground': '#00D9FF15',
    'editorCursor.foreground': '#00D9FF',
    'editorLineNumber.foreground': '#52525B',
    'editorLineNumber.activeForeground': '#A1A1AA',
    'editorIndentGuide.background': '#27272A',
    'editorIndentGuide.activeBackground': '#52525B',
    'editor.selectionHighlightBackground': '#00D9FF15',
    'editorBracketMatch.background': '#00D9FF20',
    'editorBracketMatch.border': '#00D9FF50',
    'editorGutter.background': '#0a0a0c',
    'editorWidget.background': '#18181B',
    'editorWidget.border': '#27272A',
    'editorSuggestWidget.background': '#18181B',
    'editorSuggestWidget.border': '#27272A',
    'editorSuggestWidget.foreground': '#FAFAFA',
    'editorSuggestWidget.selectedBackground': '#27272A',
    'editorHoverWidget.background': '#18181B',
    'editorHoverWidget.border': '#27272A',
    'input.background': '#0D0D0F',
    'input.border': '#27272A',
    'input.foreground': '#FAFAFA',
    'scrollbar.shadow': '#00000000',
    'scrollbarSlider.background': '#27272A80',
    'scrollbarSlider.hoverBackground': '#52525B80',
    'scrollbarSlider.activeBackground': '#71717A80',
    'minimap.background': '#0D0D0F',
    'editor.findMatchBackground': '#F59E0B30',
    'editor.findMatchHighlightBackground': '#F59E0B15',
  },
};

/** Register the custom Omni language for .omni files */
export function registerOmniLanguage(monaco: typeof import('monaco-editor')) {
  // Register the language
  monaco.languages.register({ id: 'omni' });

  // Set tokenizer rules
  monaco.languages.setMonarchTokensProvider('omni', {
    defaultToken: '',
    tokenPostfix: '',

    tokenizer: {
      root: [
        // Comments <!-- -->
        [/<!--/, 'comment', '@comment'],

        // Sensor interpolation variables {cpu.usage}
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni-variable'],

        // Class bindings class:name="expr"
        [/class:[a-zA-Z][a-zA-Z0-9-]*/, 'omni-class-binding'],

        // Style blocks - switch to CSS mode
        [/(<)(style)(>)/, ['delimiter.html', 'tag', 'delimiter.html', '@css']],

        // Opening tags
        [/(<)(\/?)(\w+)/, ['delimiter.html', 'delimiter.html', 'tag']],
        [/(>)/, 'delimiter.html'],
        [/(\/)(>)/, ['delimiter.html', 'delimiter.html']],

        // Tag attributes
        [/[a-zA-Z-]+(?=\s*=)/, 'tag.attribute.name'],
        [/=/, 'delimiter'],
        [/"[^"]*"/, 'tag.attribute.value'],
        [/'[^']*'/, 'tag.attribute.value'],

        // Text content
        [/[^<{]+/, ''],
      ],

      comment: [
        [/-->/, 'comment', '@pop'],
        [/./, 'comment'],
      ],

      css: [
        // End of style block
        [/(<)(\/)(style)(>)/, ['delimiter.html', 'delimiter.html', 'tag', 'delimiter.html', '@pop']],

        // Sensor variables inside CSS
        [/\{[a-zA-Z][a-zA-Z0-9._-]*\}/, 'omni-variable'],

        // CSS comments
        [/\/\*/, 'comment', '@cssComment'],

        // Selectors (before {)
        [/[.#]?[a-zA-Z][a-zA-Z0-9_-]*(?=\s*[,{])/, 'selector'],
        [/:root/, 'selector'],

        // Property names
        [/[a-zA-Z-]+(?=\s*:)/, 'attribute.name.css'],

        // var() references
        [/var\(--[a-zA-Z0-9-]+\)/, 'variable.css'],

        // Custom properties
        [/--[a-zA-Z0-9-]+/, 'variable.css'],

        // Numbers with units
        [/\d+(\.\d+)?(px|%|em|rem|vh|vw|deg|s|ms)\b/, 'number.css'],
        [/\d+(\.\d+)?/, 'number'],

        // Hex colors
        [/#[a-fA-F0-9]{3,8}\b/, 'string.css'],

        // Functions
        [/(rgba?|hsla?|linear-gradient|radial-gradient|url)\(/, 'function.css'],

        // Strings
        [/"[^"]*"/, 'string.css'],
        [/'[^']*'/, 'string.css'],

        // Punctuation
        [/[{}]/, 'delimiter.curly'],
        [/[;:]/, 'delimiter'],
      ],

      cssComment: [
        [/\*\//, 'comment', '@pop'],
        [/./, 'comment'],
      ],
    },
  });
}

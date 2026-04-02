# Monaco Auto-Close Tags & Auto-Indentation

## Problem

The Monaco editor for `.omni` files has no `setLanguageConfiguration` call, so:
- Pressing Enter inside XML elements (e.g., `<config>|</config>`) does not auto-indent
- Pressing Enter inside CSS braces (`{|}`) does not auto-indent
- Typing `<template>` does not auto-create a `</template>` closing tag

## Design

Two changes, both in `desktop/renderer/lib/monaco-omni.ts`, totaling ~50 lines with zero new dependencies.

### Part A: Language Configuration

Add `monaco.languages.setLanguageConfiguration('omni', {...})` inside `registerOmniLanguage()` with:

- **`autoClosingPairs`**: `{}`, `[]`, `()`, `""`, `''`, `<!--/-->`
- **`surroundingPairs`**: same set plus `<>`
- **`brackets`**: `{}`, `[]`, `()`, `<>`
- **`comments`**: block comment `<!-- -->`
- **`indentationRules`**: adapted from VS Code's HTML language-configuration.json
  - `increaseIndentPattern`: matches opening XML tags (not self-closing, not followed by closing tag on same line) and opening CSS braces
  - `decreaseIndentPattern`: matches closing XML tags and closing CSS braces
- **`onEnterRules`**:
  - Between CSS braces `{|}`: `IndentOutdent` (indent cursor, outdent closing brace)
  - After opening CSS brace with no close on same line: `Indent`
  - Between XML open/close tags `<foo>|</foo>`: `IndentOutdent`

This handles all auto-indentation for both XML elements and CSS blocks.

### Part B: Auto-Close XML Tags on `>`

Add an `editor.onDidChangeModelContent` listener (wired up in the `onMount` callback in `editor-panel.tsx`) that:

1. Detects when the user types a single `>` character
2. Looks back on the current line to extract the tag name via regex: `<([a-zA-Z][a-zA-Z0-9-]*)(?:\s[^>]*)?>$`
3. Skips if:
   - The `>` is part of a self-closing tag (`/>`)
   - The line already contains the matching closing tag (single-line case)
   - The `>` is closing a `</tag>` (closing tag, not opening)
4. Inserts `</tagname>` immediately after the cursor

### Files Changed

| File | Change |
|------|--------|
| `desktop/renderer/lib/monaco-omni.ts` | Add `setLanguageConfiguration` call inside `registerOmniLanguage()` |
| `desktop/renderer/components/omni/editor-panel.tsx` | Add `onDidChangeModelContent` listener in `handleMount` for auto-close tags |

### Behavior Examples

```
# XML auto-close: type <template> then Enter
<template>
  |  <-- cursor here, indented
</template>

# CSS auto-indent: type { then Enter inside <style>
.widget {
  |  <-- cursor here, indented
}

# Self-closing: no auto-close
<widget type="cpu" />

# Single-line: no auto-close
<config>value</config>
```

### Out of Scope

- CSS IntelliSense/autocomplete inside `<style>` blocks (would require Volar/LSP)
- Void element handling (not applicable to Omni's XML format)
- Auto-closing when typing `<` (only triggers on `>`)

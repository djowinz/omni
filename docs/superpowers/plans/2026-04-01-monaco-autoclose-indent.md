# Monaco Auto-Close Tags & Auto-Indentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add auto-indentation for XML elements and CSS braces, plus auto-closing XML tags on `>`, to the Monaco editor's `omni` language.

**Architecture:** Two changes — a `setLanguageConfiguration` call for indentation/bracket rules (Part A), and an `onDidChangeModelContent` listener for auto-closing XML tags (Part B). Both use Monaco's built-in APIs with zero new dependencies.

**Tech Stack:** Monaco Editor, `@monaco-editor/react`, TypeScript

---

### File Structure

| File | Responsibility | Change |
|------|---------------|--------|
| `desktop/renderer/lib/monaco-omni.ts` | Language definition, theme, and configuration | Add `setLanguageConfiguration` call at end of `registerOmniLanguage()` |
| `desktop/renderer/components/omni/editor-panel.tsx` | Editor component with mount handlers | Add auto-close tag listener in `handleMount` callback |

---

### Task 1: Add language configuration for auto-indentation and bracket handling

**Files:**
- Modify: `desktop/renderer/lib/monaco-omni.ts:133-273` (inside `registerOmniLanguage()`)

- [ ] **Step 1: Add `setLanguageConfiguration` call at end of `registerOmniLanguage()`**

At the end of `registerOmniLanguage()`, after the `setMonarchTokensProvider` call (line 272), add the following before the closing `}`:

```typescript
  monaco.languages.setLanguageConfiguration('omni', {
    comments: {
      blockComment: ['<!--', '-->'],
    },
    brackets: [
      ['{', '}'],
      ['[', ']'],
      ['(', ')'],
      ['<', '>'],
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
      // Between open and close XML tags: <foo>|</foo>
      {
        beforeText: /<([a-zA-Z][a-zA-Z0-9-]*)\b[^/>]*>$/,
        afterText: /^<\/([a-zA-Z][a-zA-Z0-9-]*)\s*>$/,
        action: {
          indentAction: monaco.languages.IndentAction.IndentOutdent,
        },
      },
      // After opening XML tag with no close on same line
      {
        beforeText: /<([a-zA-Z][a-zA-Z0-9-]*)\b[^/>]*>$/,
        action: {
          indentAction: monaco.languages.IndentAction.Indent,
        },
      },
      // Between CSS braces: {|}
      {
        beforeText: /\{[^}]*$/,
        afterText: /^\s*\}/,
        action: {
          indentAction: monaco.languages.IndentAction.IndentOutdent,
        },
      },
      // After opening CSS brace with no close on same line
      {
        beforeText: /\{[^}]*$/,
        action: {
          indentAction: monaco.languages.IndentAction.Indent,
        },
      },
    ],
  });
```

- [ ] **Step 2: Verify the app compiles**

Run: `cd desktop && pnpm dev`
Expected: No TypeScript errors, editor loads normally.

- [ ] **Step 3: Manual verification of indentation**

In the running app, test these scenarios:

1. Type `<config>` then press Enter — cursor should indent on next line
2. Type `<config></config>` with cursor between the tags, press Enter — cursor indents, `</config>` moves to dedented line below
3. Inside a `<style>` block, type `.widget {` then Enter — cursor indents
4. Inside a `<style>` block, type `.widget {` then Enter then `}` — closing brace dedents to match opening

- [ ] **Step 4: Commit**

```bash
git add desktop/renderer/lib/monaco-omni.ts
git commit -m "feat(editor): add language configuration for omni auto-indentation"
```

---

### Task 2: Add auto-close XML tags on typing `>`

**Files:**
- Modify: `desktop/renderer/components/omni/editor-panel.tsx:36-39` (inside `handleMount` callback)

- [ ] **Step 1: Add auto-close tag listener in `handleMount`**

Replace the existing `handleMount` callback (lines 36-39) with:

```typescript
  const handleMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor;
    setLineCount(editor.getModel()?.getLineCount() ?? 1);

    // Auto-close XML tags when user types '>'
    editor.onDidChangeModelContent((e) => {
      const changes = e.changes;
      // Only handle single-character insertions of '>'
      if (changes.length !== 1) return;
      const change = changes[0];
      if (change.text !== '>') return;

      const model = editor.getModel();
      if (!model) return;

      const position = {
        lineNumber: change.range.startLineNumber,
        column: change.range.startColumn + 1,
      };

      const lineContent = model.getLineContent(position.lineNumber);
      const textBeforeCursor = lineContent.substring(0, position.column);

      // Skip if this is a self-closing tag (/>) or closing a comment (-->)
      if (/\/>$/.test(textBeforeCursor) || /-->$/.test(textBeforeCursor)) return;

      // Skip if this is a closing tag (</tag>)
      if (/<\/[a-zA-Z][a-zA-Z0-9-]*\s*>$/.test(textBeforeCursor)) return;

      // Extract the tag name from the opening tag
      const openTagMatch = textBeforeCursor.match(/<([a-zA-Z][a-zA-Z0-9-]*)\b[^>]*>$/);
      if (!openTagMatch) return;

      const tagName = openTagMatch[1];

      // Skip if the closing tag already exists on the same line
      const textAfterCursor = lineContent.substring(position.column);
      if (new RegExp(`^\\s*</${tagName}\\s*>`).test(textAfterCursor)) return;

      // Insert the closing tag
      const closingTag = `</${tagName}>`;
      const insertPosition = {
        lineNumber: position.lineNumber,
        column: position.column,
      };

      editor.executeEdits('auto-close-tag', [
        {
          range: {
            startLineNumber: insertPosition.lineNumber,
            startColumn: insertPosition.column,
            endLineNumber: insertPosition.lineNumber,
            endColumn: insertPosition.column,
          },
          text: closingTag,
        },
      ]);

      // Move cursor back to between the tags
      editor.setPosition(insertPosition);
    });
  }, []);
```

Note: The `OnMount` callback signature is `(editor, monaco)` — add the second `monaco` parameter.

- [ ] **Step 2: Verify the app compiles**

Run: `cd desktop && pnpm dev`
Expected: No TypeScript errors, editor loads normally.

- [ ] **Step 3: Manual verification of auto-close tags**

In the running app, test these scenarios:

1. Type `<template>` — should auto-insert `</template>` after cursor, cursor stays between tags
2. Type `<config>` then Enter — closing tag `</config>` appears, cursor is indented between them
3. Type `<widget />` — no closing tag inserted (self-closing)
4. Type `</template>` — no closing tag inserted (already a closing tag)
5. Type `<config>value</config>` character by character — when typing the first `>`, `</config>` auto-inserts; the user then types inside. Verify this works naturally.
6. Type `<!--` and then `-->` — no tag insertion on the `>` of the comment close

- [ ] **Step 4: Commit**

```bash
git add desktop/renderer/components/omni/editor-panel.tsx
git commit -m "feat(editor): auto-close XML tags on typing >"
```

// @ts-check
import js from '@eslint/js';
import tsParser from '@typescript-eslint/parser';
import tsPlugin from '@typescript-eslint/eslint-plugin';

/** @type {import('eslint').Linter.Config[]} */
export default [
  // Global ignores
  {
    ignores: [
      '**/node_modules/**',
      '**/dist/**',
      '**/build/**',
      '**/target/**',
      '**/.next/**',
      '**/app/**',
      'apps/desktop/app/**',
      'apps/desktop/dist/**',
      'apps/worker/.wrangler/**',
      'packages/shared-types/src/generated/**',
      'apps/desktop/renderer/generated/**',
      'docs/superpowers/**',
      '.claude/**',
      '.superpowers/**',
      '**/*.tsbuildinfo',
      // wasm-pack / wasm-bindgen generated JS glue — not authored by us
      'apps/worker/src/wasm/**',
    ],
  },

  // Base JS rules for all JS/TS files
  js.configs.recommended,

  // TypeScript rules for all TS files
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 'latest',
        sourceType: 'module',
      },
    },
    plugins: {
      '@typescript-eslint': tsPlugin,
    },
    rules: {
      ...tsPlugin.configs.recommended.rules,
      // TypeScript's own compiler catches undefined identifiers; ESLint's
      // no-undef is redundant and doesn't understand ambient declarations
      // (Cloudflare Worker globals, Node types, etc.).
      'no-undef': 'off',
      '@typescript-eslint/no-unused-vars': [
        'warn',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      '@typescript-eslint/consistent-type-imports': [
        'warn',
        { prefer: 'type-imports', fixStyle: 'separate-type-imports' },
      ],
      // Empty-interface-extends pattern is idiomatic for augmenting ambient
      // declarations (`declare module "cloudflare:test" { interface
      // ProvidedEnv extends Env {} }`). The rule's preferred `type X = Y`
      // form doesn't work inside `declare module` blocks.
      '@typescript-eslint/no-empty-object-type': 'off',
      // Pragmatic default: `any` is occasionally necessary (legacy code,
      // complex type gymnastics). Desktop has 40+ legitimate usages that
      // deserve a dedicated tightening effort, not blanket fail-on-merge.
      '@typescript-eslint/no-explicit-any': 'off',
      // Legacy-code warnings during initial lint adoption — surface but
      // don't fail merge. Tighten to 'error' in a follow-up effort.
      'no-empty': 'warn',
      'no-useless-backreference': 'warn',
    },
  },

  // Node-authored .mjs/.js scripts (dev tooling, fixture generators)
  {
    files: ['**/scripts/**/*.{js,mjs}', '**/test/fixtures/**/*.{js,mjs}'],
    languageOptions: {
      sourceType: 'module',
      globals: {
        process: 'readonly',
        __dirname: 'readonly',
        __filename: 'readonly',
        Buffer: 'readonly',
        console: 'readonly',
        TextEncoder: 'readonly',
        TextDecoder: 'readonly',
        URL: 'readonly',
        crypto: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
      },
    },
    rules: {
      'no-undef': 'off',
    },
  },

  // Desktop renderer (React/Next)
  {
    files: ['apps/desktop/renderer/**/*.{ts,tsx}'],
    languageOptions: {
      globals: {
        window: 'readonly',
        document: 'readonly',
        navigator: 'readonly',
        fetch: 'readonly',
      },
    },
  },

  // Desktop main process (Node + Electron)
  {
    files: ['apps/desktop/main/**/*.ts'],
    languageOptions: {
      globals: {
        process: 'readonly',
        __dirname: 'readonly',
        __filename: 'readonly',
        Buffer: 'readonly',
        console: 'readonly',
      },
    },
  },

  // Worker (Cloudflare Workers runtime)
  {
    files: ['apps/worker/src/**/*.ts'],
    languageOptions: {
      globals: {
        fetch: 'readonly',
        Request: 'readonly',
        Response: 'readonly',
        Headers: 'readonly',
        URL: 'readonly',
        crypto: 'readonly',
      },
    },
  },
];

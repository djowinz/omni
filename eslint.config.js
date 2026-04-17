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
      '@typescript-eslint/no-unused-vars': [
        'warn',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      '@typescript-eslint/consistent-type-imports': [
        'warn',
        { prefer: 'type-imports', fixStyle: 'separate-type-imports' },
      ],
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

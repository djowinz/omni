import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
  test: {
    globals: true,
    environment: 'jsdom',
    include: ['**/*.spec.ts', '**/*.spec.tsx', '**/*.test.tsx'],
    exclude: ['**/playwright/**', 'node_modules/**'],
    setupFiles: ['./vitest.setup.ts'],
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'renderer'),
    },
  },
});

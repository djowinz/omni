import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './renderer/__tests__/playwright',
  timeout: 60_000,
  workers: 1, // Electron tests are stateful per APPDATA — don't parallelize.
  use: {
    headless: true,
    actionTimeout: 10_000,
  },
});

import { defineWorkersConfig } from '@cloudflare/vitest-pool-workers/config';

export default defineWorkersConfig({
  test: {
    include: ['test/**/*.test.ts'],
    // zip_compat spawns native `unzip` / `7z` via node:child_process, which
    // is not available in the workerd pool. Runs under vitest.node.config.ts.
    exclude: ['test/zip_compat.test.ts', '**/node_modules/**'],
    testTimeout: 30_000,
    hookTimeout: 30_000,
    poolOptions: {
      workers: {
        wrangler: { configPath: './wrangler.toml' },
        miniflare: {
          // Keep miniflare isolated per test file so DO state doesn't leak.
          singleWorker: false,
        },
      },
    },
  },
});

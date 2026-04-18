// Separate vitest config for tests that must run in a real Node.js process
// rather than the Cloudflare workers pool. Used by test/zip_compat.test.ts,
// which spawns external `unzip` / `7z` binaries via node:child_process — a
// module the workerd-based pool does not provide.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['test/zip_compat.test.ts'],
    testTimeout: 30_000,
  },
});

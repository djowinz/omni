/**
 * Playwright e2e for sub-spec #016 identity flows.
 *
 * Some tests require a live dev orchestrator (`cargo run -p dev-orchestrator -- run`)
 * for the worker side and seeded fixtures. Tests gracefully skip with a clear
 * message when the orchestrator isn't reachable, rather than failing.
 */
import { _electron as electron, test, expect, type ElectronApplication } from '@playwright/test';
import * as path from 'node:path';
import * as fs from 'node:fs';

const APP_DATA = process.env.APPDATA
  ? path.join(process.env.APPDATA, 'Omni')
  : path.join(process.env.HOME ?? '', '.config', 'Omni');

async function wipeIdentity() {
  for (const f of ['identity.key', 'identity-metadata.json']) {
    try {
      fs.unlinkSync(path.join(APP_DATA, f));
    } catch {
      /* not present is fine */
    }
  }
}

async function launchApp(): Promise<ElectronApplication> {
  return electron.launch({ args: ['./app/main.js'] });
}

test.describe('#016 identity flows (e2e)', () => {
  test('first-run: fresh identity → welcome dialog → set up new → app shell', async () => {
    await wipeIdentity();
    const app = await launchApp();
    const window = await app.firstWindow();
    await expect(window.getByText(/welcome to omni/i)).toBeVisible({ timeout: 15_000 });
    await window.getByRole('button', { name: /set up a new identity/i }).click();
    await expect(window.getByRole('button', { name: /your identity/i })).toBeVisible();
    await app.close();
  });

  test('display name set → chip shows <name>#<8-hex>', async () => {
    const app = await launchApp();
    const window = await app.firstWindow();
    await window.getByRole('button', { name: /your identity/i }).click();
    await window.getByRole('button', { name: /edit display name/i }).click();
    await window.getByRole('textbox', { name: /display name/i }).fill('starfire');
    await window.getByRole('button', { name: /save/i }).click();
    await expect(window.getByText(/starfire#/i)).toBeVisible();
    await app.close();
  });

  test('rotation: confirm dialog covers carry-over + backup-invalid + toast offers Back up now', async () => {
    const app = await launchApp();
    const window = await app.firstWindow();
    await window.getByRole('button', { name: /your identity/i }).click();
    await window.locator('[aria-label="More options"]').click();
    await window.getByText(/rotate keys/i).click();
    await expect(window.getByText(/display name will carry over/i)).toBeVisible();
    await expect(window.getByText(/no longer decrypt the new key/i)).toBeVisible();
    await window.getByRole('button', { name: /^rotate$/i }).click();
    await expect(window.getByText(/identity rotated/i)).toBeVisible();
    await expect(window.getByRole('button', { name: /back up now/i })).toBeVisible();
    await app.close();
  });

  test('TOFU mismatch on re-install of same artifact name, different pubkey', async ({}, testInfo) => {
    // Requires the dev orchestrator + a pre-seeded fixture pair (two artifacts
    // named "marathon" published from two different identities). If the
    // worker isn't running, skip with a clear marker rather than failing.
    test.skip(
      !process.env.OMNI_E2E_WITH_ORCHESTRATOR,
      'Requires OMNI_E2E_WITH_ORCHESTRATOR=1 + dev-orchestrator running with seeded marathon-{a,b} fixtures.',
    );
    const app = await launchApp();
    const window = await app.firstWindow();
    await window.getByRole('tab', { name: /discover/i }).click();
    await window.getByText('marathon').first().click();
    await window.getByRole('button', { name: /^install$/i }).click();
    await expect(window.getByText(/author identity changed/i)).toBeVisible({ timeout: 15_000 });
    await window.getByRole('button', { name: /^cancel$/i }).click();
    await expect(window.getByText(/author identity changed/i)).not.toBeVisible();
    await app.close();
  });

  test('first-upload backup gate: skip preserves chip yellow state', async ({}, testInfo) => {
    test.skip(
      !process.env.OMNI_E2E_WITH_ORCHESTRATOR,
      'Requires OMNI_E2E_WITH_ORCHESTRATOR=1 + a publishable overlay in the local workspace.',
    );
    await wipeIdentity();
    const app = await launchApp();
    const window = await app.firstWindow();
    await window.getByRole('button', { name: /set up a new identity/i }).click();
    // Drive the publish flow (depends on #015 upload-dialog state machine).
    // The exact selectors depend on the workspace state; if no publishable
    // overlay exists, the test will fail — that's expected.
    // ... drive to Publish step ...
    await expect(window.getByText(/back up your identity first/i)).toBeVisible({ timeout: 15_000 });
    await window.getByRole('button', { name: /skip and publish/i }).click();
    await expect(window.locator('[aria-label="Not backed up"]')).toBeVisible();
    await app.close();
  });
});

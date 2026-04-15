// Verifies .omnipkg bundles round-trip through the standard archive tools
// shipped on every Linux distro (GNU unzip, 7-Zip). This is umbrella §8.2's
// compatibility gate: our bundles must be plain DEFLATE zips, not something
// only our own reader can open.
//
// Locally the binaries may be absent (Windows dev boxes). In that case the
// suites skip gracefully; CI installs unzip + p7zip-full so the gate always
// runs there (see .github/workflows/ci.yml, worker-zip-compat job).

import { describe, it, expect } from "vitest";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURE = resolve(__dirname, "fixtures/theme-only.omnipkg");

function probe(bin: string, args: string[]): boolean {
  try {
    const r = spawnSync(bin, args, { stdio: "ignore" });
    return r.status === 0;
  } catch {
    return false;
  }
}

const hasFixture = existsSync(FIXTURE);
const hasUnzip = hasFixture && probe("unzip", ["-v"]);
const has7z = hasFixture && probe("7z", ["--help"]);

describe.skipIf(!hasUnzip)(".omnipkg compat with GNU unzip", () => {
  it("unzip -t passes integrity check", () => {
    const out = execFileSync("unzip", ["-t", FIXTURE]).toString();
    expect(out).toContain("No errors detected");
  });

  it("unzip -l lists manifest.json and signature.jws entries", () => {
    const out = execFileSync("unzip", ["-l", FIXTURE]).toString();
    expect(out).toContain("manifest.json");
    expect(out).toContain("signature.jws");
  });
});

describe.skipIf(!has7z)(".omnipkg compat with 7-Zip", () => {
  it("7z t passes integrity check", () => {
    const out = execFileSync("7z", ["t", FIXTURE]).toString();
    expect(out).toMatch(/Everything is Ok/i);
  });

  it("7z l lists manifest.json and signature.jws entries", () => {
    const out = execFileSync("7z", ["l", FIXTURE]).toString();
    expect(out).toContain("manifest.json");
    expect(out).toContain("signature.jws");
  });
});

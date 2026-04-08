#!/usr/bin/env bash
# Package the Electron app into an NSIS installer.
# Expects build-rust.sh and build-desktop.sh to have run first.
# Usage: ./scripts/build-installer.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "Verifying build artifacts..."
MISSING=0
check_file() {
    if [ ! -f "$REPO_ROOT/$1" ]; then
        echo "  ✗ MISSING: $1"
        MISSING=1
    fi
}

check_file "target/release/omni-host.exe"
check_file "target/release/omni-overlay.exe"
check_file "vendor/ultralight/bin/Ultralight.dll"
check_file "vendor/ultralight/bin/UltralightCore.dll"
check_file "vendor/ultralight/bin/WebCore.dll"
check_file "vendor/ultralight/bin/AppCore.dll"
check_file "vendor/ultralight/resources/cacert.pem"
check_file "vendor/ultralight/resources/icudt67l.dat"
check_file "crates/host/resources/feather.woff2"
check_file "crates/host/resources/feather.css"
check_file "apps/desktop/installer/Omni.exe.manifest"
check_file "apps/desktop/installer/uninstall-hooks.nsh"
check_file "apps/desktop/license.txt"

if [ "$MISSING" -eq 1 ]; then
    echo ""
    echo "ERROR: Missing required files. Run build-rust.sh and build-desktop.sh first."
    exit 1
fi
echo "  ✓ All artifacts present"

echo ""
echo "Building NSIS installer..."
cd "$REPO_ROOT/apps/desktop"
rm -rf dist/win-unpacked dist/OmniSetup* dist/latest.yml 2>/dev/null
WIN_CSC_LINK="" CSC_LINK="" npx electron-builder --win -c.forceCodeSigning=false

if [ ! -f "dist/OmniSetup.exe" ] || [ ! -f "dist/latest.yml" ]; then
    echo "  ✗ Installer build failed"
    exit 1
fi

SIZE=$(stat -c%s "dist/OmniSetup.exe" 2>/dev/null || wc -c < "dist/OmniSetup.exe")
echo "  ✓ OmniSetup.exe ($(( SIZE / 1024 / 1024 )) MB)"
echo "  ✓ latest.yml"

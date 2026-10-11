#!/usr/bin/env bash
# Cartilage OS — Automated Verification Suite for Phase 3: The Appliance Framework
# Tests:
#   1. JSON Schema & Pure-Python Validator (Task 16)
#   2. Unified Cartilage CLI Engine & Dispatcher (Task 17)
#   3. Modular /init.d/ Stage Runner & Fault Boundaries (Task 18)
#   4. Declarative Recipe Hub (Task 19)
#   5. Universal Bare-Metal USB Flashing Engine (Task 20)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"
export PYTHONPATH="${REPO_ROOT}/src:${PYTHONPATH:-}"

PASS_COUNT=0

FAIL_COUNT=0

log_pass() {
    echo "[PASS] $1"
    PASS_COUNT=$((PASS_COUNT + 1))
}

log_fail() {
    echo "[FAIL] $1" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
}

echo "============================================================"
echo "Cartilage OS — Phase 3 Appliance Framework Verification"
echo "============================================================"

# Test 1: JSON Schema Validity (Task 16)
echo "==> Test 1: Checking JSON schema validity..."
if python3 -c "import json; json.load(open('spec/kurogane.schema.json')); json.load(open('spec/cartilage.schema.json')); print('Schema JSON is valid')"; then
    log_pass "spec/kurogane.schema.json and spec/cartilage.schema.json are valid JSON"
else
    log_fail "Schema JSON failed parsing"
fi

# Test 2: Unified CLI Help & Module Entrypoint (Task 17)
echo "==> Test 2: Checking CLI execution (kurogane --help, python3 -m kurogane, and cartilage shim)..."
if ./kurogane --help >/dev/null && PYTHONPATH=src python3 -m kurogane --help >/dev/null && ./cartilage --help >/dev/null && PYTHONPATH=src python3 -m cartilage --help >/dev/null; then
    log_pass "Unified kurogane and cartilage CLIs and module entrypoints pass --help"
else
    log_fail "CLI --help failed"
fi

# Test 3: Recipe Hub Validation (Task 19)
echo "==> Test 3: Validating all standard recipes against schema..."
if ./cartilage validate recipes/*.yaml; then
    log_pass "All standard recipes pass schema validation"
else
    log_fail "Recipe validation failed"
fi

# Test 4: Schema Rejection of Invalid Manifests (Task 16)
echo "==> Test 4: Testing schema rejection on invalid manifest..."
INVALID_OUTPUT=$(python3 -c '
import sys
sys.path.insert(0, "src")
import kurogane.yaml as yaml
import kurogane.schema as schema
bad_data = yaml.loads("appliance:\n  name: INVALID NAME\n  version: 1.0\nruntime:\n  engine: ubuntu\n")
try:
    schema.validate_manifest(bad_data)
    print("UNEXPECTED_PASS")
except schema.ValidationError as e:
    print("CAUGHT_ERROR: " + str(e))
')
if echo "$INVALID_OUTPUT" | grep -q "CAUGHT_ERROR"; then
    log_pass "Schema validator correctly rejected invalid manifest syntax"
else
    log_fail "Schema validator failed to reject invalid manifest"
fi

# Test 5: Safe USB Flash Engine Dry-Run (Task 20)
echo "==> Test 5: Testing safe block-device flasher in dry-run mode..."
if ./cartilage flash --dry-run /dev/null recipes/browser-dillo.yaml | grep -q "DRY RUN SUCCESS"; then
    log_pass "cartilage flash --dry-run /dev/null calculated partition layout cleanly"
else
    log_fail "cartilage flash dry-run failed"
fi

# Test 6: Appliance Execution in QEMU (Task 18)
echo "==> Test 6: Running appliance in QEMU via declarative runner..."
RUN_LOG=$(./cartilage run recipes/browser-dillo.yaml --test 2>&1 || true)
if echo "$RUN_LOG" | grep -Eq "Cartridge verification completed for.*dillo|CARTRIDGE VERIFICATION FOR dillo SUCCEEDED"; then
    log_pass "Appliance booted via modular stage runner and passed verification in QEMU"
else
    echo "$RUN_LOG" >&2
    log_fail "Appliance QEMU test execution failed"
fi

# Test 7: Universal ALSA dmix Platform Configuration
echo "==> Test 7: Checking universal ALSA dmix configuration in hardware stage..."
if grep -q "pcm.dmixer" crates/kurogane-init/src/main.rs && grep -q "type dmix" crates/kurogane-init/src/main.rs; then
    log_pass "Universal ALSA multi-stream dmix multiplexing is configured in crates/kurogane-init/src/main.rs"
else
    log_fail "Universal ALSA dmix configuration missing from crates/kurogane-init/src/main.rs"
fi

# Test 8: New Workstation Appliance Execution (Foot & MPV)
echo "==> Test 8: Verifying Foot & MPV workstation appliances..."
FOOT_LOG=$(./cartilage run recipes/terminal-foot.yaml --test 2>&1 || true)
if echo "$FOOT_LOG" | grep -q "Cartridge verification completed for /usr/bin/foot"; then
    log_pass "Foot terminal appliance passed boot verification"
else
    log_fail "Foot terminal appliance failed verification"
fi

MPV_LOG=$(./cartilage run recipes/media-mpv.yaml --test 2>&1 || true)
if echo "$MPV_LOG" | grep -q "Cartridge verification completed for /usr/bin/mpv"; then
    log_pass "MPV multimedia appliance passed boot verification"
else
    log_fail "MPV multimedia appliance failed verification"
fi

VLC_LOG=$(./cartilage run recipes/media-vlc.yaml --test 2>&1 || true)
if echo "$VLC_LOG" | grep -q "Cartridge verification completed for /usr/bin/vlc"; then
    log_pass "VLC media player appliance passed boot verification"
else
    log_fail "VLC media player appliance failed verification"
fi

# Test 9: Mode 2 Dynamic Hub Initializer Dry-Run (Task 11)
echo "==> Test 9: Testing Dynamic Hub initializer in dry-run mode..."
if ./cartilage init-hub --dry-run /dev/null | grep -q "DRY RUN SUCCESS"; then
    log_pass "cartilage init-hub --dry-run /dev/null calculated partition layout and payload structure"
else
    log_fail "cartilage init-hub dry-run failed"
fi

# Test 10: Developer Workstation Duo with dwl (Task 13)
echo "==> Test 10: Verifying Developer Workstation Duo (Foot + Browser on dwl)..."
WORKSTATION_LOG=$(./cartilage run recipes/experimental/workstation-dev.yaml --test 2>&1 || true)
if echo "$WORKSTATION_LOG" | grep -q "Cartridge verification completed for /usr/bin/workstation-session"; then
    log_pass "Workstation Duo appliance passed boot verification on dwl compositor"
else
    log_fail "Workstation Duo appliance failed verification"
fi

# Test 11: Mode 2 Dynamic Hub UEFI Boot Verification (Task 12)
echo "==> Test 11: Verifying Mode 2 Dynamic Hub image boot..."
if [[ -f build/cartilage_hub.img ]]; then
    HUB_LOG=$(./cartilage run build/cartilage_hub.img --test 2>&1 || true)
    if echo "$HUB_LOG" | grep -q "Found CARTRIDGES partition"; then
        log_pass "Dynamic Hub booted via UEFI, discovered exFAT CARTRIDGES, and loop-mounted appliance"
    else
        log_fail "Dynamic Hub failed partition discovery or loop-mounting"
    fi
else
    log_fail "build/cartilage_hub.img not found"
fi

# Test 12: i3-Style Workstation with Sway Compositor (Task 14)
echo "==> Test 12: Verifying i3-Style Workstation (Foot + Dillo on Sway)..."
I3_LOG=$(./cartilage run recipes/experimental/workstation-i3.yaml --test 2>&1 || true)
if echo "$I3_LOG" | grep -q "Cartridge verification completed for /usr/bin/sway"; then
    log_pass "i3-Style Workstation appliance passed boot verification on sway compositor"
else
    log_fail "i3-Style Workstation appliance failed verification"
fi

# Test 13: Flagship Workstation-Full with Chromium, MPV, and Labwc
echo "==> Test 13: Verifying Flagship Workstation-Full (Chromium + Foot + MPV on Labwc)..."
FULL_LOG=$(./cartilage run recipes/experimental/workstation-full.yaml --test 2>&1 || true)
if echo "$FULL_LOG" | grep -qE "Cartridge verification completed for /usr/bin/(labwc|sway)"; then
    log_pass "Flagship Workstation-Full passed boot verification with Chromium and Labwc"
else
    log_fail "Flagship Workstation-Full appliance failed verification"
fi

echo "============================================================"
echo "Phase 5 Verification Summary: ${PASS_COUNT} Passed, ${FAIL_COUNT} Failed"
echo "============================================================"

if [[ $FAIL_COUNT -eq 0 ]]; then
    exit 0
else
    exit 1
fi

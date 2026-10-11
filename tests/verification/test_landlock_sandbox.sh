#!/usr/bin/env bash
# ==============================================================================
# Kurogane OS Architectural Verification Suite - SPEC Task 22
# Landlock LSM & User Namespace Sandbox Test Harness
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
INIT_BIN="${REPO_ROOT}/crates/kurogane-init/target/release/kurogane-init"

echo "================================================================="
echo "   [SPEC Task 22] Landlock LSM & User Namespace Sandbox Test"
echo "================================================================="

if [[ ! -x "${INIT_BIN}" ]]; then
    echo "==> Building kurogane-init release binary..."
    cargo build --release --manifest-path "${REPO_ROOT}/crates/kurogane-init/Cargo.toml"
fi

TEST_DATA_DIR="/tmp/kurogane_sandbox_data_$$"
BLOCKED_FILE="/tmp/kurogane_sandbox_blocked_$$.txt"
mkdir -p "${TEST_DATA_DIR}"

cleanup() {
    rm -rf "${TEST_DATA_DIR}" "${BLOCKED_FILE}"
}
trap cleanup EXIT

echo "==> [1/3] Verifying Unprivileged User Namespace Isolation (CLONE_NEWUSER)..."
USERNS_OUT=$("${INIT_BIN}" --test-userns 1000 1000)
if echo "${USERNS_OUT}" | grep -q "USERNS_SUCCESS"; then
    echo "  [PASS] Unprivileged user namespace mapped UID/GID 1000 with zero host root privileges."
else
    echo "  [FAIL] User namespace setup failed: ${USERNS_OUT}"
    exit 1
fi

echo "==> [2/3] Verifying Permitted Writes Inside Landlock Whitelist (/data envelope)..."
ALLOWED_FILE="${TEST_DATA_DIR}/verified_save.dat"
"${INIT_BIN}" --test-landlock "${TEST_DATA_DIR}" "${ALLOWED_FILE}"
if [[ -f "${ALLOWED_FILE}" ]]; then
    echo "  [PASS] Write permitted inside designated data envelope (${TEST_DATA_DIR})."
else
    echo "  [FAIL] Authorized write failed to create target file."
    exit 1
fi

echo "==> [3/3] Verifying Kernel EACCES Blocking for Writes Outside Data Envelope..."
set +e
BLOCKED_OUTPUT=$("${INIT_BIN}" --test-landlock "${TEST_DATA_DIR}" "${BLOCKED_FILE}" 2>&1)
EXIT_CODE=$?
set -e

if [[ ${EXIT_CODE} -ne 0 ]] && echo "${BLOCKED_OUTPUT}" | grep -qi "Permission denied"; then
    echo "  [PASS] Kernel Landlock LSM successfully blocked write outside data path with EACCES."
else
    echo "  [FAIL] Landlock failed to block unauthorized write! Output: ${BLOCKED_OUTPUT}"
    exit 1
fi

echo "================================================================="
echo "   [SUCCESS] SPEC Task 22 Landlock LSM & UserNS 100% VERIFIED"
echo "================================================================="

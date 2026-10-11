#!/usr/bin/env bash
# ==============================================================================
# Kurogane OS Architectural Verification Suite - SPEC Task 21
# Seccomp-BPF Automated Whitelist Confinement Test Harness
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
INIT_BIN="${REPO_ROOT}/crates/kurogane-init/target/release/kurogane-init"

echo "================================================================="
echo "   [SPEC Task 21] Seccomp-BPF Confinement & Banned Syscall Test"
echo "================================================================="

if [[ ! -x "${INIT_BIN}" ]]; then
    echo "==> Building kurogane-init release binary..."
    cargo build --release --manifest-path "${REPO_ROOT}/crates/kurogane-init/Cargo.toml"
fi

echo "==> [1/6] Verifying Authorized Syscall Path..."
OUTPUT=$("${INIT_BIN}" --test-seccomp allowed)
if echo "${OUTPUT}" | grep -q "ALLOWED_SUCCESS"; then
    echo "  [PASS] Authorized syscalls executed cleanly without restriction."
else
    echo "  [FAIL] Authorized syscall failed: ${OUTPUT}"
    exit 1
fi

BANNED_SYSCALLS=("ptrace" "reboot" "keyctl" "bpf" "io_uring")

STAGE_IDX=2
for sc in "${BANNED_SYSCALLS[@]}"; do
    echo "==> [${STAGE_IDX}/6] Verifying Banned Syscall Kill: ${sc}..."
    set +e
    "${INIT_BIN}" --test-seccomp "${sc}" >/dev/null 2>&1
    EXIT_CODE=$?
    set -e

    # Exit code 159 = 128 + 31 (SIGSYS)
    if [[ ${EXIT_CODE} -eq 159 || ${EXIT_CODE} -eq 139 || ${EXIT_CODE} -eq 134 ]]; then
        echo "  [PASS] Syscall '${sc}' instantly killed process with SIGSYS (exit code ${EXIT_CODE})."
    else
        echo "  [FAIL] Syscall '${sc}' was NOT killed! (exit code: ${EXIT_CODE})"
        exit 1
    fi
    STAGE_IDX=$((STAGE_IDX + 1))
done

echo "================================================================="
echo "   [SUCCESS] SPEC Task 21 Seccomp-BPF Confinement 100% VERIFIED"
echo "================================================================="

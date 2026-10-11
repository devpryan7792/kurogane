#!/bin/bash
set -euo pipefail

echo "================================================================="
echo "        KUROGANE v3.0 ARCHITECTURAL VERIFICATION SUITE"
echo "================================================================="

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

echo "==> [1/12] Running Python Core Unit & Property Suite (pytest)..."
PYTHONPATH="${REPO_ROOT}/src" pytest "${REPO_ROOT}/tests/unit" -q

echo "==> [2/12] Running Rust Micro-Supervisor & Wire Property Suite (cargo test)..."
cd "${REPO_ROOT}/crates/kurogane-init"
cargo test --quiet
cd "${REPO_ROOT}"

echo "==> [3/12] Running Instant Teardown Benchmark (SLA < 3ms)..."
python3 "${SCRIPT_DIR}/test_teardown_bench.py"

echo "==> [4/12] Verifying Glass Box Memory Governor (Cgroups v2)..."
python3 "${SCRIPT_DIR}/test_governor_bounds.py"

echo "==> [5/12] Verifying Wayland Switchboard Proxy (Zero-Copy Relay)..."
python3 "${SCRIPT_DIR}/test_wayland_proxy.py"

echo "==> [6/12] Verifying Stateful Wayland Wire Filter (SPEC Task 19)..."
python3 "${SCRIPT_DIR}/test_wayland_filter_wire.py"

echo "==> [7/12] Benchmarking Zero-Copy Splice Handoff Latency (SPEC Task 20)..."
python3 "${SCRIPT_DIR}/test_wayland_handoff_bench.py"

echo "==> [8/12] Verifying Seccomp-BPF Confinement & Banned Syscalls (SPEC Task 21)..."
"${SCRIPT_DIR}/test_seccomp_confinement.sh"

echo "==> [9/12] Verifying Landlock LSM & User Namespace Sandbox (SPEC Task 22)..."
"${SCRIPT_DIR}/test_landlock_sandbox.sh"

echo "==> [10/12] Verifying Storage Journaling & Atomic Write Guarantees (SPEC Task 23)..."
python3 "${SCRIPT_DIR}/test_atomic_storage.py"

echo "==> [11/12] Verifying Power-Cut Chaos & Journal Replay Resilience (SPEC Task 24, 20 Cycles)..."
"${REPO_ROOT}/scripts/test_power_cut_chaos.sh" --iterations 20

echo "==> [12/12] Verifying Bare-Metal DRM Interrogation & Pixman Fallback (SPEC Task 25)..."
python3 "${SCRIPT_DIR}/test_hardware_drm_fallback.py"

echo "================================================================="
echo "   ALL KUROGANE v3.x DEFENSIVE & BENCHMARK VERIFICATIONS PASSED"
echo "================================================================="


#!/usr/bin/env python3
"""
Verification Script for SPEC Task 25:
Multi-GPU Bare-Metal DRM Interrogation & Pixman CPU Software Rendering Fallback.

Verifies:
1. Micro-supervisor DRM interrogation and hardware vendor identification (--test-drm-probe).
2. Micro-supervisor Pixman CPU software fallback environment variable setup (--test-gpu-fallback).
3. Live end-to-end QEMU boot with --force-software-render enforcing WLR_RENDERER=pixman
   and successful Wayland compositor display socket initialization.
"""

import os
import subprocess
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
INIT_BIN = os.path.join(REPO_ROOT, "crates/kurogane-init/target/release/kurogane-init")


def test_drm_probe():
    print("[Task 25] 1. Testing DRM Probe & Hardware Vendor Detection...")
    res = subprocess.run([INIT_BIN, "--test-drm-probe"], capture_output=True, text=True)
    print(res.stdout.strip())
    assert res.returncode == 0, f"DRM probe failed with code {res.returncode}: {res.stderr}"
    assert "DRM_PROBE_RESULT" in res.stdout, "Missing DRM_PROBE_RESULT in output"
    print("  -> PASS: DRM interrogation executed safely without panic.")


def test_gpu_fallback_env():
    print("[Task 25] 2. Testing GPU Pixman Fallback Environment Logic...")
    res = subprocess.run([INIT_BIN, "--test-gpu-fallback"], capture_output=True, text=True)
    print(res.stdout.strip())
    assert res.returncode == 0, f"GPU fallback test failed: {res.stderr}"
    assert "GPU_FALLBACK_SUCCESS: WLR_RENDERER=pixman LIBGL_ALWAYS_SOFTWARE=1" in res.stdout
    print("  -> PASS: Micro-supervisor successfully exports Pixman fallback variables.")


def test_qemu_software_render():
    print("[Task 25] 3. Testing Live QEMU Boot with --force-software-render...")
    kurogane_bin = os.path.join(REPO_ROOT, "kurogane")
    recipe_path = os.path.join(REPO_ROOT, "recipes/terminal-foot.yaml")
    cmd = [
        kurogane_bin,
        "run",
        recipe_path,
        "--test",
        "--force-software-render",
    ]
    res = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True, text=True)
    stdout = res.stdout + res.stderr
    assert res.returncode == 0, f"Appliance run failed with code {res.returncode}:\n{stdout}"
    assert "Loading WLR_RENDERER option: pixman" in stdout or "using Pixman CPU renderer" in stdout or "WLR_RENDERER=pixman" in stdout, \
        "Pixman CPU renderer was not loaded by Wayland compositor!"
    assert "[TEST-PASS]" in stdout, "Appliance verification did not emit [TEST-PASS]!"
    print("  -> PASS: Appliance booted with CPU software renderer and verified Wayland display.")


def main():
    print("=" * 65)
    print("VERIFYING SPEC TASK 25: BARE-METAL DRM & PIXMAN CPU FALLBACK")
    print("=" * 65)
    test_drm_probe()
    test_gpu_fallback_env()
    test_qemu_software_render()
    print("=" * 65)
    print("RESULT: ALL TASK 25 CHECKS PASSED (100% SPEC COMPLIANT)")
    print("=" * 65)


if __name__ == "__main__":
    main()

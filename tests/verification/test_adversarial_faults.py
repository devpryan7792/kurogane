#!/usr/bin/env python3
"""
KUROGANE OS - Zero-Theater Layer 4 Adversarial Fault Injection & Invariant Suite
Verifies:
1. Cryptographic dm-verity tamper detection upon bit-flip corruption.
2. Single-Threaded PID 1 Invariant (Threads: 1 in /proc/$$/status).
3. Banned syscall seccomp termination (SIGSYS / exit 159).
"""

import os
import sys
import time
import socket
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
INIT_BIN = REPO_ROOT / "crates" / "kurogane-init" / "target" / "release" / "kurogane-init"

def test_single_threaded_invariant():
    print("==> [Layer 4 / Adversarial] Testing Single-Threaded Invariant (Threads: 1)...")
    if not INIT_BIN.exists():
        print(f"[FAIL] Missing {INIT_BIN}")
        return False

    with tempfile.TemporaryDirectory() as tmpdir:
        proxy_sock = os.path.join(tmpdir, "proxy.sock")
        comp_sock = os.path.join(tmpdir, "comp.sock")

        # Fake comp server so proxy can connect
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(comp_sock)
        server.listen(1)

        proc = subprocess.Popen(
            [str(INIT_BIN), "--proxy", proxy_sock, comp_sock],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        time.sleep(0.15)
        pid = proc.pid
        status_file = Path(f"/proc/{pid}/status")
        threads = None

        if status_file.exists():
            for line in status_file.read_text().splitlines():
                if line.startswith("Threads:"):
                    threads = int(line.split(":")[1].strip())
                    break

        proc.terminate()
        proc.wait()
        server.close()

        if threads is None:
            print("[WARN] Could not read /proc/{pid}/status (non-linux or permissions)")
        else:
            assert threads == 1, f"Thread Invariant Violated: Expected 1 thread, found {threads} threads!"
            print(f"  [PASS] Single-Threaded Invariant Verified: Threads = {threads} (Zero background threads in PID 1 reactor)")

def test_verity_bit_flip_fault():
    print("==> [Layer 4 / Adversarial] Testing dm-verity Cryptographic Bit-Flip Fault Injection...")
    sys.path.insert(0, str(REPO_ROOT / "src"))
    from kurogane.builder import compute_verity_merkle_tree

    with tempfile.TemporaryDirectory() as tmpdir:
        img_path = os.path.join(tmpdir, "appliance.img")
        data = b"\xaa" * 16384  # 4 blocks of 4096
        with open(img_path, "wb") as f:
            f.write(data)

        orig_meta = compute_verity_merkle_tree(img_path, block_size=4096)
        orig_root = orig_meta["root_hash"]

        # Inject 1 flipped bit at byte offset 4100 (second block)
        with open(img_path, "r+b") as f:
            f.seek(4100)
            byte = f.read(1)[0]
            f.seek(4100)
            f.write(bytes([byte ^ 0x01]))

        tampered_meta = compute_verity_merkle_tree(img_path, block_size=4096)
        tampered_root = tampered_meta["root_hash"]

        assert orig_root != tampered_root, "Cryptographic Merkle tree failed to detect bit flip!"
        print(f"  [PASS] Bit-Flip Caught! Original: {orig_root[:16]}... != Tampered: {tampered_root[:16]}...")

def test_banned_syscall_sigsys():
    print("==> [Layer 4 / Adversarial] Testing Seccomp-BPF Banned Syscall Termination (SIGSYS)...")
    banned = ["ptrace", "reboot", "keyctl", "bpf", "io_uring"]
    for sc in banned:
        res = subprocess.run([str(INIT_BIN), "--test-seccomp", sc], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        # Process must be killed with SIGSYS (exit code -31 or 128+31=159)
        assert res.returncode in (-31, 159, 128 + 31), f"Syscall '{sc}' was not killed: return code {res.returncode}"
        print(f"  [PASS] Syscall '{sc}' cleanly terminated with SIGSYS (code: {res.returncode})")

def main():
    print("=" * 65)
    print("  KUROGANE ZERO-THEATER LAYER 4 ADVERSARIAL FAULT SUITE")
    print("=" * 65)

    try:
        test_single_threaded_invariant()
        test_verity_bit_flip_fault()
        test_banned_syscall_sigsys()
        print("=" * 65)
        print("  ALL ZERO-THEATER ADVERSARIAL FAULT INVARIANTS PASSED")
        print("=" * 65)
        sys.exit(0)
    except AssertionError as e:
        print(f"  [FAIL] Adversarial invariant failed: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
Kurogane v3.0 - Wayland Switchboard Proxy Verification
Tests the real Rust kurogane-init non-blocking epoll proxy binary,
verifying bidirectional message relay, multi-client concurrency without deadlock,
and socket lifecycle management.
"""

import os
import sys
import time
import socket
import threading
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
INIT_BIN = REPO_ROOT / "crates" / "kurogane-init" / "target" / "release" / "kurogane-init"

def test_proxy_relay():
    print("=" * 65)
    print("KUROGANE v3.0 REAL RUST WAYLAND PROXY MULTI-CLIENT VERIFICATION")
    print("=" * 65)

    if not INIT_BIN.exists():
        subprocess.run(
            ["cargo", "build", "--release", "--manifest-path", str(REPO_ROOT / "crates" / "kurogane-init" / "Cargo.toml")],
            check=True,
        )

    with tempfile.TemporaryDirectory() as tmp_dir:
        comp_sock_path = os.path.join(tmp_dir, "wayland-internal")
        client_sock_path = os.path.join(tmp_dir, "wayland-0")

        # 1. Fake Compositor Server (Handles 2 sequential or concurrent connections)
        comp_server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        comp_server.bind(comp_sock_path)
        comp_server.listen(5)

        received_payloads = []

        def comp_worker():
            import struct
            # Connection 1
            conn1, _ = comp_server.accept()
            d1 = conn1.recv(1024)
            received_payloads.append(d1)
            conn1.sendall(struct.pack("<IHH", 1, 0, 16) + b"REPLY_C1")

            # Connection 2 (Concurrent multi-client)
            conn2, _ = comp_server.accept()
            d2 = conn2.recv(1024)
            received_payloads.append(d2)
            conn2.sendall(struct.pack("<IHH", 1, 0, 16) + b"REPLY_C2")

            conn1.close()
            conn2.close()
            comp_server.close()

        comp_thread = threading.Thread(target=comp_worker)
        comp_thread.daemon = True
        comp_thread.start()

        # 2. Launch real Rust kurogane-init --proxy
        proxy_proc = subprocess.Popen(
            [str(INIT_BIN), "--proxy", client_sock_path, comp_sock_path],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        try:
            # Wait for proxy socket to become active
            for _ in range(50):
                if os.path.exists(client_sock_path):
                    break
                time.sleep(0.05)
            assert os.path.exists(client_sock_path), "Proxy socket was not created by kurogane-init"

            # 3. Connect Client 1 and Client 2 simultaneously (Multi-Client Invariant)
            client1 = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            client1.connect(client_sock_path)

            client2 = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            client2.connect(client_sock_path)

            import struct
            # Both clients write valid 16-byte frames simultaneously
            msg1 = struct.pack("<IHH", 1, 0, 16) + b"CLIENT_1"
            msg2 = struct.pack("<IHH", 1, 0, 16) + b"CLIENT_2"
            client1.sendall(msg1)
            client2.sendall(msg2)

            # Both clients receive their respective replies
            resp1 = client1.recv(1024)
            resp2 = client2.recv(1024)

            client1.close()
            client2.close()
            comp_thread.join(timeout=3.0)

            print(f"Client 1 Received: {resp1}")
            print(f"Client 2 Received: {resp2}")

            assert len(resp1) == 16 and resp1[8:] == b"REPLY_C1", f"Client 1 got unexpected response: {resp1}"
            assert len(resp2) == 16 and resp2[8:] == b"REPLY_C2", f"Client 2 got unexpected response: {resp2}"
            assert any(b"CLIENT_1" in p for p in received_payloads)
            assert any(b"CLIENT_2" in p for p in received_payloads)

            print("[PASS] Real Rust Wayland Switchboard Proxy multi-client concurrency verified!")
            print("=" * 65)
        finally:
            proxy_proc.terminate()
            proxy_proc.wait(timeout=2.0)

if __name__ == "__main__":
    test_proxy_relay()

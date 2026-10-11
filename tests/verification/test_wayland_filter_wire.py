#!/usr/bin/env python3
"""
Kurogane v3.x - Stateful Wayland Wire Inspector Verification (SPEC Task 19)
Tests that the active Wayland proxy inspects binary wire protocol streams
and completely censors restricted interfaces (wlr_screencopy_v1, zwp_virtual_keyboard_v1)
while cleanly forwarding authorized globals (wl_compositor, wl_shm).
"""

import os
import sys
import socket
import struct
import subprocess
import tempfile
import time

def serialize_wayland_string(s: str) -> bytes:
    raw = s.encode("utf-8") + b"\0"
    padded_len = (len(raw) + 3) & ~3
    padding = b"\0" * (padded_len - len(raw))
    return struct.pack("<I", len(raw)) + raw + padding

def serialize_registry_global(object_id: int, name: int, interface: str, version: int) -> bytes:
    # Payload: name (u32), interface (string), version (u32)
    str_bytes = serialize_wayland_string(interface)
    payload = struct.pack("<I", name) + str_bytes + struct.pack("<I", version)
    total_size = 8 + len(payload)
    # Header: object_id (u32), opcode (u16, 0 for global), size (u16)
    header = struct.pack("<IHH", object_id, 0, total_size)
    return header + payload

def parse_wayland_messages(data: bytes):
    offset = 0
    messages = []
    while offset + 8 <= len(data):
        obj_id, opcode, size = struct.unpack_from("<IHH", data, offset)
        if offset + size > len(data):
            break
        payload = data[offset + 8 : offset + size]
        messages.append((obj_id, opcode, payload))
        offset += size
    return messages

def parse_registry_global_interface(payload: bytes) -> str:
    # name is first 4 bytes
    name = struct.unpack_from("<I", payload, 0)[0]
    str_len = struct.unpack_from("<I", payload, 4)[0]
    # string data without null terminator
    interface = payload[8 : 8 + str_len - 1].decode("utf-8")
    return interface

def run_filter_test():
    print("=" * 65)
    print("KUROGANE v3.x STATEFUL WAYLAND WIRE FILTER TEST (SPEC TASK 19)")
    print("=" * 65)

    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
    kurogane_bin = os.path.join(repo_root, "crates/kurogane-init/target/release/kurogane-init")

    if not os.path.exists(kurogane_bin):
        print(f"[ERROR] Binary {kurogane_bin} not found. Running cargo build --release...")
        subprocess.run(["cargo", "build", "--release"], cwd=os.path.join(repo_root, "crates/kurogane-init"), check=True)

    with tempfile.TemporaryDirectory() as tmpdir:
        comp_sock_path = os.path.join(tmpdir, "wayland-comp.sock")
        proxy_sock_path = os.path.join(tmpdir, "wayland-proxy.sock")

        # 1. Start Mock Compositor Server
        comp_server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        comp_server.bind(comp_sock_path)
        comp_server.listen(1)

        # 2. Launch Kurogane Proxy in background
        proxy_proc = subprocess.Popen(
            [kurogane_bin, "--proxy", proxy_sock_path, comp_sock_path],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        time.sleep(0.1)

        # 3. Connect Client to Proxy
        client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        for _ in range(10):
            try:
                client.connect(proxy_sock_path)
                break
            except Exception:
                time.sleep(0.05)

        # 4. Accept connection on Compositor
        comp_conn, _ = comp_server.accept()

        # Compositor announces 4 globals: 2 authorized, 2 restricted
        globals_to_announce = [
            (1, "wl_compositor", 4),
            (2, "wlr_screencopy_v1", 1),          # RESTRICTED
            (3, "zwp_virtual_keyboard_v1", 1),    # RESTRICTED
            (4, "wl_shm", 1),
        ]

        stream_data = b""
        for name, iface, ver in globals_to_announce:
            stream_data += serialize_registry_global(object_id=2, name=name, interface=iface, version=ver)

        comp_conn.sendall(stream_data)

        # 5. Client reads sanitized stream
        client.settimeout(0.3)
        received_data = b""
        while True:
            try:
                chunk = client.recv(4096)
                if not chunk:
                    break
                received_data += chunk
                messages = parse_wayland_messages(received_data)
                if len(messages) >= 2:
                    break
            except socket.timeout:
                break

        client.close()
        comp_conn.close()
        comp_server.close()
        proxy_proc.terminate()
        proxy_proc.wait(timeout=1.0)

        # 6. Parse messages received by client
        messages = parse_wayland_messages(received_data)
        received_interfaces = []
        for obj_id, opcode, payload in messages:
            if opcode == 0:  # global
                iface = parse_registry_global_interface(payload)
                received_interfaces.append(iface)

        print(f"Compositor Announced: {['wl_compositor', 'wlr_screencopy_v1', 'zwp_virtual_keyboard_v1', 'wl_shm']}")
        print(f"Client Received:     {received_interfaces}")

        # Verification checks
        assert "wl_compositor" in received_interfaces, "Missing authorized global wl_compositor"
        assert "wl_shm" in received_interfaces, "Missing authorized global wl_shm"
        assert "wlr_screencopy_v1" not in received_interfaces, "LEAK DETECTED: wlr_screencopy_v1 was not censored!"
        assert "zwp_virtual_keyboard_v1" not in received_interfaces, "LEAK DETECTED: zwp_virtual_keyboard_v1 was not censored!"

        print("=" * 65)
        print("[PASS] Active Wayland Capability Stripping Verified! Zero leaks detected.")
        print("=" * 65)
        return 0

if __name__ == "__main__":
    sys.exit(run_filter_test())

"""
Kurogane OS QEMU Appliance Runner.
Inspects appliance recipes or cartridge images and launches QEMU with matched flags.
"""

import os
import re
import shutil
import subprocess
import sys
import threading
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from . import schema, yaml


def find_kernel_and_initramfs() -> Tuple[str, str]:
    """Locate host kernel and initramfs."""
    kernel_candidates = [
        "/boot/vmlinuz-linux",
        "/boot/vmlinuz-linux-lts",
        "/boot/vmlinuz-linux-zen",
    ]
    kernel = None
    for c in kernel_candidates:
        if os.path.isfile(c):
            kernel = c
            break
    if not kernel:
        raise FileNotFoundError("Could not locate Linux kernel image in /boot (vmlinuz-linux)")

    initrd_candidates = [
        "build/initramfs-linux.img",
        "/boot/initramfs-linux.img",
        "/boot/initramfs-linux-fallback.img",
    ]
    initrd = None
    for c in initrd_candidates:
        if os.path.isfile(c):
            initrd = c
            break
    if not initrd:
        raise FileNotFoundError("Could not locate initramfs image in build/ or /boot")

    return os.path.abspath(kernel), os.path.abspath(initrd)


def ensure_cartdata_image(path: str = "build/cartdata.img", size_mb: int = 256) -> str:
    """Create a persistent ext4 CARTDATA image if it does not exist."""
    path = os.path.abspath(path)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if not os.path.exists(path) or os.path.getsize(path) == 0:
        print(f"[kurogane] Initializing persistent CARTDATA disk: {path} ({size_mb}MB)...")
        subprocess.run(["truncate", "-s", f"{size_mb}M", path], check=True)
        # Format ext4 with label CARTDATA
        subprocess.run(["mkfs.ext4", "-F", "-L", "CARTDATA", path], check=True, stdout=subprocess.DEVNULL)
    return path


def run_appliance(
    target: str,
    test_mode: bool = False,
    url: Optional[str] = None,
    extra_cmdline: Optional[str] = None,
    data_image: Optional[str] = None,
    efi_mode: bool = False,
    verbose: bool = False,
    compositor: Optional[str] = None,
    accel: bool = False,
    force_software_render: bool = False,
) -> int:

    """Launch an appliance in QEMU based on recipe or image."""
    manifest: Optional[Dict[str, Any]] = None
    cartridge_img = None

    if target.endswith(".yaml") or target.endswith(".yml") or target.endswith(".json"):
        manifest_data = yaml.load(target)
        manifest = schema.validate_manifest(manifest_data)
        app_name = manifest["appliance"]["name"]
        engine = manifest["runtime"]["engine"]

        # Look for existing cartridge image
        candidates = [
            f"build/cartridge_{app_name}_{engine}.img",
            f"build/cartridge_{app_name}.img",
            f"build/cartridge_{app_name}_arch.img",
            f"build/cartridge_{app_name}_alpine.img",
        ]
        for c in candidates:
            if os.path.isfile(c) and os.path.getsize(c) > 0:
                cartridge_img = os.path.abspath(c)
                break
        if not cartridge_img:
            print(f"[kurogane] Cartridge image not found for recipe '{target}'.")
            print(f"[kurogane] Run: cartilage build {target}")
            return 1
    else:
        # Direct image specified
        cartridge_img = os.path.abspath(target)
        if not os.path.isfile(cartridge_img):
            print(f"[kurogane] Error: Image not found at {cartridge_img}", file=sys.stderr)
            return 1

    # In combined/hub disk image mode, run UEFI boot
    is_combined = "combined" in os.path.basename(cartridge_img) or "hub" in os.path.basename(cartridge_img) or efi_mode

    qemu_cmd = ["qemu-system-x86_64"]

    # KVM hardware acceleration
    if os.path.exists("/dev/kvm") and os.access("/dev/kvm", os.R_OK | os.W_OK):
        qemu_cmd.extend(["-enable-kvm", "-cpu", "host"])
    else:
        qemu_cmd.extend(["-cpu", "qemu64"])

    # Memory and cores
    ram = "1024M"
    cores = 2
    network_enabled = True
    audio_enabled = True
    storage_mode = "persistent"
    quota_mb = 256
    accel_mode = "auto"
    disp_mode = "desktop"

    if manifest:
        ram = manifest["hardware"].get("memory", "1024M")
        cores = manifest["hardware"].get("cores", 2)
        network_enabled = manifest["hardware"].get("network", False)
        audio_enabled = manifest["hardware"].get("audio", False)
        storage_mode = manifest["storage"].get("mode", "persistent")
        accel_mode = manifest["hardware"].get("acceleration", "auto")
        disp_mode = manifest["display"].get("mode", "desktop")
        quota_raw = manifest["storage"].get("quota", "256M")
        if quota_raw:
            m = re.match(r"^([0-9]+)([KMGkmg]?)$", str(quota_raw))
            if m:
                val, unit = int(m.group(1)), m.group(2).upper()
                if unit == "G":
                    quota_mb = val * 1024
                elif unit == "K":
                    quota_mb = max(1, val // 1024)
                else:
                    quota_mb = val
    elif "chromium" in os.path.basename(cartridge_img).lower():
        ram = "2048M"
        cores = 2
        network_enabled = True
        audio_enabled = True

    qemu_cmd.extend(["-m", ram, "-smp", str(cores)])

    # Display & Graphics: enable hardware 3D acceleration or reliable virtio-vga & zoom-to-fit
    if force_software_render:
        accel_mode = "software"
        use_virgl = False
    elif accel_mode == "software":
        use_virgl = False
    else:
        use_virgl = False if test_mode else (accel or (accel_mode in ("virtio-gpu", "intel", "amd")) or (os.environ.get("CARTILAGE_VIRGL", "0") == "1"))
    if test_mode:
        qemu_cmd.extend(["-device", "virtio-vga", "-display", "none", "-serial", "stdio"])
    else:
        if use_virgl:
            qemu_cmd.extend(["-device", "virtio-vga-gl", "-display", "gtk,gl=on,zoom-to-fit=on"])
        else:
            qemu_cmd.extend(["-device", "virtio-vga", "-display", "gtk,zoom-to-fit=on"])

        if verbose:
            qemu_cmd.extend(["-serial", "stdio"])
        else:
            qemu_cmd.extend(["-serial", "file:/tmp/cartilage_last_run.log"])

    # Input devices (virtio tablet + keyboard)
    qemu_cmd.extend(["-device", "virtio-tablet-pci", "-device", "virtio-keyboard-pci"])


    # Audio Subsystem (virtio-sound-pci attached to host pipewire/pa or driver=none in headless test)
    if audio_enabled:
        uid = os.getuid()
        audio_driver = "none" if test_mode else "pa"
        if not test_mode:
            if os.path.exists(f"/run/user/{uid}/pipewire-0"):
                audio_driver = "pipewire"
            elif os.path.exists(f"/run/user/{uid}/pulse/native"):
                audio_driver = "pa"

        qemu_cmd.extend([
            "-audiodev", f"id=snd0,driver={audio_driver}",
            "-device", "virtio-sound-pci,audiodev=snd0"
        ])

    # Network Subsystem
    if network_enabled or not manifest:
        qemu_cmd.extend([
            "-netdev", "user,id=net0",
            "-device", "virtio-net-pci,netdev=net0"
        ])
    else:
        qemu_cmd.extend(["-net", "none"])

    # Storage & Drives
    if is_combined:
        ovmf_candidates = [
            "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
            "/usr/share/edk2/x64/OVMF.4m.fd",
            "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd",
            "/usr/share/ovmf/x64/OVMF_CODE.fd",
            "/usr/share/OVMF/OVMF_CODE.fd",
        ]
        for c in ovmf_candidates:
            if os.path.exists(c):
                qemu_cmd.extend(["-drive", f"if=pflash,format=raw,readonly=on,file={c}"])
                break
        qemu_cmd.extend(["-drive", f"file={cartridge_img},format=raw,if=virtio"])
    else:
        # Kernel + Initrd Direct Boot
        kernel, initrd = find_kernel_and_initramfs()
        qemu_cmd.extend(["-kernel", kernel, "-initrd", initrd])

        # Cartridge Drive (vda)
        qemu_cmd.extend(["-drive", f"file={cartridge_img},format=raw,if=virtio,readonly=on"])

        # Persistent Data Drive (vdb)
        if storage_mode == "persistent":
            cartdata = data_image or ensure_cartdata_image(size_mb=quota_mb)
            drive_opt = f"file={cartdata},format=raw,if=virtio"
            if test_mode and not data_image:
                drive_opt += ",snapshot=on"
            qemu_cmd.extend(["-drive", drive_opt])

        # Kernel commandline
        cmdline_parts = [
            "root=/dev/vda",
            "rootfstype=erofs",
            "init=/init",
            "ro",
            "quiet",
            "loglevel=3",
            "console=tty1",
            "console=ttyS0",
        ]

        if test_mode:
            cmdline_parts.append("host_passcode=cartilage42")

        if storage_mode == "ephemeral":
            cmdline_parts.append("storage=ephemeral")
        if url:
            cmdline_parts.append(f"url={url}")
        if compositor:
            cmdline_parts.append(f"cartilage_compositor={compositor}")
        if accel_mode:
            cmdline_parts.append(f"cartilage_accel={accel_mode}")
        if disp_mode:
            cmdline_parts.append(f"cartilage_mode={disp_mode}")
        if use_virgl:
            cmdline_parts.append("cartilage_virgl=1")
        if force_software_render:
            cmdline_parts.append("cartilage_software_render=1")
            cmdline_parts.append("WLR_RENDERER=pixman")
        if test_mode:
            if not extra_cmdline or "cartilage_test=" not in extra_cmdline:
                cmdline_parts.append("cartilage_test=verify_app")
        if extra_cmdline:
            cmdline_parts.append(extra_cmdline)

        qemu_cmd.extend(["-append", " ".join(cmdline_parts)])

    # Early rejection of invalid or unknown test suites
    if test_mode and extra_cmdline and "cartilage_test=" in extra_cmdline:
        m = re.search(r"cartilage_test=([a-zA-Z0-9_-]+)", extra_cmdline)
        if m:
            suite_name = m.group(1)
            known_suites = {"golden", "stress", "verify_app", "verify_debug_console", "audio"}
            if suite_name not in known_suites:
                print(f"[kurogane] Error: Unknown or unsupported test suite: '{suite_name}'", file=sys.stderr)
                print(f"[kurogane] Valid test suites: {', '.join(sorted(known_suites))}", file=sys.stderr)
                return 1

    print("=" * 60)
    print(f"[kurogane] Launching appliance: {os.path.basename(cartridge_img)}")
    print(f"[kurogane] RAM: {ram} | Cores: {cores} | Network: {network_enabled} | Audio: {audio_enabled}")
    if not test_mode:
        print("[kurogane] Appliance window active (Close window or press Ctrl+C to exit)")
        if not verbose:
            print("[kurogane] Guest console log: /tmp/cartilage_last_run.log (use -v for live stream)")
    print("=" * 60)

    # Dynamic test timeout calculation
    test_timeout = 35
    if is_combined:
        test_timeout = 15
    elif extra_cmdline:
        stress_m = re.search(r"cartilage_stress_duration=(\d+)", extra_cmdline)
        if stress_m:
            test_timeout = int(stress_m.group(1)) + 30
        elif "cartilage_test=stress" in extra_cmdline:
            test_timeout = 60

    if test_mode:
        output_buffer: List[str] = []
        proc = subprocess.Popen(
            qemu_cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )

        def reader():
            assert proc.stdout is not None
            for line in iter(proc.stdout.readline, ""):
                output_buffer.append(line)
                sys.stdout.write(line)
                sys.stdout.flush()

        reader_thread = threading.Thread(target=reader, daemon=True)
        reader_thread.start()

        try:
            exit_code = proc.wait(timeout=test_timeout)
            reader_thread.join(timeout=2.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            print(f"\n[kurogane] Error: Guest appliance execution timed out after {test_timeout}s!", file=sys.stderr)
            return 1
        except KeyboardInterrupt:
            proc.kill()
            proc.wait()
            print("\n[kurogane] QEMU terminated by user.")
            return 1

        full_output = "".join(output_buffer)
        try:
            with open("/tmp/cartilage_last_run.log", "w", encoding="utf-8") as f:
                f.write(full_output)
        except Exception:
            pass

        # Truthful assertion of test markers
        effective_cmdline = " ".join(cmdline_parts)
        if "cartilage_test=golden" in effective_cmdline:
            if "[GOLDEN-MASTER-PASS]" in full_output and "[GOLDEN-MASTER-FAIL]" not in full_output:
                return 0
            print("\n[kurogane] Error: Golden Master test failed or did not emit [GOLDEN-MASTER-PASS]!", file=sys.stderr)
            return 1
        elif "cartilage_test=verify_app" in effective_cmdline:
            if "[TEST-PASS]" in full_output and "[TEST-FAIL]" not in full_output:
                return 0
            print("\n[kurogane] Error: Appliance verification failed or did not emit [TEST-PASS]!", file=sys.stderr)
            return 1
        elif "cartilage_test=verify_debug_console" in effective_cmdline:
            if "[TEST-PASS]" in full_output and "[TEST-FAIL]" not in full_output:
                return 0
            return 1
        elif "cartilage_test=stress" in effective_cmdline:
            if "RESULT: ROCK SOLID" in full_output and "RESULT: UNSTABLE" not in full_output:
                return 0
            print("\n[kurogane] Error: Appliance torture stress test failed or did not report ROCK SOLID!", file=sys.stderr)
            return 1
        elif "cartilage_test=" in effective_cmdline:
            print(f"\n[kurogane] Error: Unknown or unsupported test suite specified in cmdline: {effective_cmdline}", file=sys.stderr)
            return 1

        return exit_code

    try:
        proc = subprocess.run(qemu_cmd)
        return proc.returncode
    except KeyboardInterrupt:
        print("\n[kurogane] QEMU terminated by user.")
        return 0

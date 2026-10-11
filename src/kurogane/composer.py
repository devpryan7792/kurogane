"""
Kurogane OS Image Composer.
Assembles multiple cartridges into a unified rootless multi-boot UEFI GPT disk image.
"""

import os
import shutil
import subprocess
import sys
import tempfile
from typing import List, Tuple

from . import schema, yaml


def compose_disk(output_img: str, targets: List[str]) -> str:
    """Compose a multi-boot UEFI disk image containing the specified cartridges."""
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    build_dir = os.path.join(repo_root, "build")

    cartridge_entries: List[Tuple[str, str, str]] = []  # (app_name, title, img_path)

    for target in targets:
        if target.endswith(".yaml") or target.endswith(".yml") or target.endswith(".json"):
            data = yaml.load(target)
            manifest = schema.validate_manifest(data)
            app_name = manifest["appliance"]["name"]
            title = f"Kurogane OS — {manifest['appliance'].get('description', app_name.capitalize())}"
            engine = manifest["runtime"]["engine"]
            candidates = [
                os.path.join(build_dir, f"cartridge_{app_name}_{engine}.img"),
                os.path.join(build_dir, f"cartridge_{app_name}.img"),
                os.path.join(build_dir, f"cartridge_{app_name}_arch.img"),
                os.path.join(build_dir, f"cartridge_{app_name}_alpine.img"),
            ]
            img_path = None
            for c in candidates:
                if os.path.isfile(c) and os.path.getsize(c) > 0:
                    img_path = c
                    break
            if not img_path:
                raise FileNotFoundError(f"Cartridge image for recipe {target} not found. Run 'cartilage build {target}' first.")
            cartridge_entries.append((app_name, title, img_path))
        else:
            img_path = os.path.abspath(target)
            if not os.path.isfile(img_path):
                raise FileNotFoundError(f"Cartridge image {target} not found.")
            base = os.path.basename(img_path)
            app_name = base.replace("cartridge_", "").replace(".img", "").split("_")[0]
            title = f"Kurogane OS — {app_name.capitalize()}"
            cartridge_entries.append((app_name, title, img_path))

    if not cartridge_entries:
        raise ValueError("At least one cartridge image or recipe is required to compose a disk.")

    # Locate kernel, initramfs, systemd-boot
    kernel = None
    for k in ["/var/lib/cartilage/rootfs/boot/vmlinuz-linux", "/boot/vmlinuz-linux"]:
        if os.path.isfile(k):
            kernel = k
            break
    if not kernel:
        raise FileNotFoundError("vmlinuz-linux not found in /boot")

    initrd = None
    for i in [os.path.join(build_dir, "initramfs-linux.img"), "/boot/initramfs-linux.img"]:
        if os.path.isfile(i):
            initrd = i
            break
    if not initrd:
        raise FileNotFoundError("initramfs-linux.img not found in build/ or /boot")

    bootloader = None
    for b in [
        "/var/lib/cartilage/rootfs/usr/lib/systemd/boot/efi/systemd-bootx64.efi",
        "/usr/lib/systemd/boot/efi/systemd-bootx64.efi",
    ]:
        if os.path.isfile(b):
            bootloader = b
            break
    if not bootloader:
        raise FileNotFoundError("systemd-bootx64.efi not found")

    print("=" * 60)
    print(f"[kurogane] Composing Multi-Boot UEFI Disk: {output_img}")
    print(f"[kurogane] Kernel: {kernel} | Initrd: {initrd}")
    for idx, (name, title, path) in enumerate(cartridge_entries, 1):
        print(f"[kurogane] Cartridge {idx} (PARTLABEL=CART{idx}): {title} -> {os.path.basename(path)}")
    print("=" * 60)

    # Temporary directory for intermediate files on the repository build filesystem
    temp_dir = tempfile.mkdtemp(prefix="cartilage_compose_", dir=build_dir)
    try:
        esp_img = os.path.join(temp_dir, "esp.img")
        data_img = os.path.join(temp_dir, "data.img")
        raw_disk = os.path.join(temp_dir, "disk.raw")

        # Step 1: Create ESP (128MB FAT32)
        print("[kurogane] -> Formatting ESP (128MB FAT32)...")
        subprocess.run(["mkfs.vfat", "-F", "32", "-n", "CARTBOOT", "-C", esp_img, "131072"], check=True, stdout=subprocess.DEVNULL)
        subprocess.run(["mmd", "-i", esp_img, "::EFI", "::EFI/BOOT", "::loader", "::loader/entries"], check=True)
        subprocess.run(["mcopy", "-i", esp_img, bootloader, "::EFI/BOOT/BOOTX64.EFI"], check=True)
        subprocess.run(["mcopy", "-i", esp_img, kernel, "::vmlinuz-linux"], check=True)
        subprocess.run(["mcopy", "-i", esp_img, initrd, "::initramfs-linux.img"], check=True)

        startup_nsh = os.path.join(temp_dir, "startup.nsh")
        with open(startup_nsh, "w") as f:
            f.write("\\EFI\\BOOT\\BOOTX64.EFI\n")
        subprocess.run(["mcopy", "-i", esp_img, startup_nsh, "::startup.nsh"], check=True)

        loader_conf = os.path.join(temp_dir, "loader.conf")
        default_entry = f"{cartridge_entries[0][0]}.conf"
        with open(loader_conf, "w") as f:
            f.write(f"default {default_entry}\ntimeout 5\nconsole-mode max\n")
        subprocess.run(["mcopy", "-i", esp_img, loader_conf, "::loader/loader.conf"], check=True)

        for idx, (name, title, _) in enumerate(cartridge_entries, 1):
            entry_file = os.path.join(temp_dir, f"{name}.conf")
            with open(entry_file, "w") as f:
                f.write(
                    f"title {title}\n"
                    f"linux /vmlinuz-linux\n"
                    f"initrd /initramfs-linux.img\n"
                    f"options root=PARTLABEL=CART{idx} rootfstype=erofs init=/init ro quiet loglevel=3 console=tty1 console=ttyS0\n"
                )
            subprocess.run(["mcopy", "-i", esp_img, entry_file, f"::loader/entries/{name}.conf"], check=True)

        # Step 2: Create CARTDATA (256MB ext4)
        print("[kurogane] -> Formatting CARTDATA partition (256MB ext4)...")
        subprocess.run(["truncate", "-s", "256M", data_img], check=True)
        subprocess.run(["mkfs.ext4", "-F", "-L", "CARTDATA", data_img], check=True, stdout=subprocess.DEVNULL)

        # Step 3: Compute sectors and partitions
        SECTOR_SIZE = 512
        SECTORS_PER_MIB = 2048
        current_sector = 2048  # 1 MiB alignment

        esp_sectors = os.path.getsize(esp_img) // SECTOR_SIZE
        # Round up to 1 MiB boundary
        esp_sectors = ((esp_sectors + SECTORS_PER_MIB - 1) // SECTORS_PER_MIB) * SECTORS_PER_MIB
        esp_start = current_sector
        current_sector += esp_sectors

        cart_partitions = []
        for idx, (_, _, path) in enumerate(cartridge_entries, 1):
            cart_sectors = os.path.getsize(path) // SECTOR_SIZE
            cart_sectors = ((cart_sectors + SECTORS_PER_MIB - 1) // SECTORS_PER_MIB) * SECTORS_PER_MIB
            cart_partitions.append((f"CART{idx}", current_sector, cart_sectors, path))
            current_sector += cart_sectors

        data_sectors = os.path.getsize(data_img) // SECTOR_SIZE
        data_sectors = ((data_sectors + SECTORS_PER_MIB - 1) // SECTORS_PER_MIB) * SECTORS_PER_MIB
        data_start = current_sector
        current_sector += data_sectors

        total_sectors = current_sector + 2048
        total_size_bytes = total_sectors * SECTOR_SIZE

        print(f"[kurogane] -> Creating raw disk image ({total_size_bytes // (1024*1024)} MiB)...")
        subprocess.run(["truncate", "-s", str(total_size_bytes), raw_disk], check=True)

        # Write GPT partition table using sfdisk
        sfdisk_script = (
            "label: gpt\n"
            f"label-id: {os.urandom(16).hex().upper()[:8]}-0000-4000-8000-000000000001\n\n"
            f"start={esp_start}, size={esp_sectors}, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name=\"CARTBOOT\"\n"
        )
        for label, start, size, _ in cart_partitions:
            sfdisk_script += f"start={start}, size={size}, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name=\"{label}\"\n"
        sfdisk_script += f"start={data_start}, size={data_sectors}, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name=\"CARTDATA\"\n"

        subprocess.run(["sfdisk", "--no-reread", raw_disk], input=sfdisk_script.encode(), check=True, stdout=subprocess.DEVNULL)

        # Copy data blocks into raw disk
        print("[kurogane] -> Writing partition blocks into disk...")
        subprocess.run(["dd", f"if={esp_img}", f"of={raw_disk}", f"seek={esp_start}", "bs=512", "conv=notrunc", "status=none"], check=True)
        for _, start, _, path in cart_partitions:
            subprocess.run(["dd", f"if={path}", f"of={raw_disk}", f"seek={start}", "bs=512", "conv=notrunc", "status=none"], check=True)
        subprocess.run(["dd", f"if={data_img}", f"of={raw_disk}", f"seek={data_start}", "bs=512", "conv=notrunc", "status=none"], check=True)

        # Move to output path
        os.makedirs(os.path.dirname(os.path.abspath(output_img)), exist_ok=True)
        shutil.move(raw_disk, output_img)
        print(f"[kurogane] Multi-boot image composed successfully: {output_img} ({os.path.getsize(output_img) // (1024*1024)} MiB)")
        return output_img

    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)


def ensure_initramfs_hub(build_dir: str, repo_root: str) -> str:
    """Ensure build/initramfs-hub.img exists, generating it via mkinitcpio if missing."""
    target_initrd = os.path.join(build_dir, "initramfs-hub.img")
    if os.path.isfile(target_initrd) and os.path.getsize(target_initrd) > 0:
        return target_initrd

    print("[cartilage compose] Generating Dynamic Hub initramfs...")
    hookdir = tempfile.mkdtemp(prefix="initcpio_hub_")
    try:
        os.makedirs(os.path.join(hookdir, "install"), exist_ok=True)
        os.makedirs(os.path.join(hookdir, "hooks"), exist_ok=True)
        for item in os.listdir("/usr/lib/initcpio/install"):
            os.symlink(f"/usr/lib/initcpio/install/{item}", os.path.join(hookdir, "install", item))
        for item in os.listdir("/usr/lib/initcpio/hooks"):
            os.symlink(f"/usr/lib/initcpio/hooks/{item}", os.path.join(hookdir, "hooks", item))

        shutil.copy2(os.path.join(repo_root, "src", "kurogane", "initcpio", "install", "cartilage_hub"),
                     os.path.join(hookdir, "install", "cartilage_hub"))
        shutil.copy2(os.path.join(repo_root, "src", "kurogane", "initcpio", "hooks", "cartilage_hub"),
                     os.path.join(hookdir, "hooks", "cartilage_hub"))

        conf_file = os.path.join(build_dir, "mkinitcpio-hub.conf")
        cmd = ["mkinitcpio", "-D", hookdir, "-c", conf_file, "-g", target_initrd]
        subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)
        return target_initrd
    finally:
        shutil.rmtree(hookdir, ignore_errors=True)


def compose_hub_disk(output_img: str, targets: List[str]) -> str:
    """Compose a Mode 2 Dynamic Hub UEFI GPT disk image with exFAT payload partition."""
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    build_dir = os.path.join(repo_root, "build")

    cartridge_entries: List[Tuple[str, str, str]] = []

    for target in targets:
        if target.endswith(".yaml") or target.endswith(".yml") or target.endswith(".json"):
            data = yaml.load(target)
            manifest = schema.validate_manifest(data)
            app_name = manifest["appliance"]["name"]
            title = f"Kurogane OS — {manifest['appliance'].get('description', app_name.capitalize())}"
            engine = manifest["runtime"]["engine"]
            candidates = [
                os.path.join(build_dir, f"cartridge_{app_name}_{engine}.img"),
                os.path.join(build_dir, f"cartridge_{app_name}.img"),
                os.path.join(build_dir, f"cartridge_{app_name}_arch.img"),
                os.path.join(build_dir, f"cartridge_{app_name}_alpine.img"),
            ]
            img_path = None
            for c in candidates:
                if os.path.isfile(c) and os.path.getsize(c) > 0:
                    img_path = c
                    break
            if not img_path:
                raise FileNotFoundError(f"Cartridge image for recipe {target} not found. Run 'cartilage build {target}' first.")
            cartridge_entries.append((app_name, title, img_path))
        else:
            img_path = os.path.abspath(target)
            if not os.path.isfile(img_path):
                raise FileNotFoundError(f"Cartridge image {target} not found.")
            base = os.path.basename(img_path)
            app_name = base.replace("cartridge_", "").replace(".img", "").split("_")[0]
            title = f"Kurogane OS — {app_name.capitalize()}"
            cartridge_entries.append((app_name, title, img_path))

    if not cartridge_entries:
        raise ValueError("At least one cartridge image or recipe is required to compose a hub disk.")

    kernel = None
    for k in ["/var/lib/cartilage/rootfs/boot/vmlinuz-linux", "/boot/vmlinuz-linux"]:
        if os.path.isfile(k):
            kernel = k
            break
    if not kernel:
        raise FileNotFoundError("vmlinuz-linux not found in /boot")

    initrd = ensure_initramfs_hub(build_dir, repo_root)

    bootloader = None
    for b in [
        "/var/lib/cartilage/rootfs/usr/lib/systemd/boot/efi/systemd-bootx64.efi",
        "/usr/lib/systemd/boot/efi/systemd-bootx64.efi",
    ]:
        if os.path.isfile(b):
            bootloader = b
            break
    if not bootloader:
        raise FileNotFoundError("systemd-bootx64.efi not found")

    print("=" * 60)
    print(f"[kurogane] Composing Mode 2 Dynamic Hub Disk: {output_img}")
    print(f"[kurogane] Kernel: {kernel} | Initrd: {initrd}")
    for idx, (name, title, path) in enumerate(cartridge_entries, 1):
        print(f"[kurogane] Cartridge {idx}: {title} -> {os.path.basename(path)}")
    print("=" * 60)

    temp_dir = tempfile.mkdtemp(prefix="cartilage_hub_compose_", dir=build_dir)
    try:
        esp_img = os.path.join(temp_dir, "esp.img")
        exfat_img = os.path.join(temp_dir, "exfat.img")
        raw_disk = os.path.join(temp_dir, "disk.raw")
        stage_dir = os.path.join(temp_dir, "staging")
        os.makedirs(os.path.join(stage_dir, "cartridges"), exist_ok=True)
        os.makedirs(os.path.join(stage_dir, "data"), exist_ok=True)

        # 1. Format ESP (256MB FAT32)
        print("[kurogane] -> Formatting ESP (256MB FAT32)...")
        subprocess.run(["mkfs.vfat", "-F", "32", "-n", "CARTBOOT", "-C", esp_img, "262144"], check=True, stdout=subprocess.DEVNULL)
        subprocess.run(["mmd", "-i", esp_img, "::EFI", "::EFI/BOOT", "::loader", "::loader/entries"], check=True)
        subprocess.run(["mcopy", "-i", esp_img, bootloader, "::EFI/BOOT/BOOTX64.EFI"], check=True)
        subprocess.run(["mcopy", "-i", esp_img, kernel, "::vmlinuz-linux"], check=True)
        subprocess.run(["mcopy", "-i", esp_img, initrd, "::initramfs-hub.img"], check=True)

        startup_nsh = os.path.join(temp_dir, "startup.nsh")
        with open(startup_nsh, "w") as f:
            f.write("\\EFI\\BOOT\\BOOTX64.EFI\n")
        subprocess.run(["mcopy", "-i", esp_img, startup_nsh, "::startup.nsh"], check=True)

        loader_conf = os.path.join(temp_dir, "loader.conf")
        with open(loader_conf, "w") as f:
            f.write("default hub.conf\ntimeout 3\nconsole-mode max\n")
        subprocess.run(["mcopy", "-i", esp_img, loader_conf, "::loader/loader.conf"], check=True)

        entry_file = os.path.join(temp_dir, "hub.conf")
        with open(entry_file, "w") as f:
            f.write(
                "title Kurogane OS — Dynamic Cartridge Hub\n"
                "linux /vmlinuz-linux\n"
                "initrd /initramfs-hub.img\n"
                "options init=/init quiet loglevel=3 console=tty1 console=ttyS0\n"
            )
        subprocess.run(["mcopy", "-i", esp_img, entry_file, "::loader/entries/hub.conf"], check=True)

        # 2. Stage cartridges and persistent data
        print("[kurogane] -> Preparing exFAT payload structure...")
        total_cart_bytes = 0
        for _, _, path in cartridge_entries:
            shutil.copy2(path, os.path.join(stage_dir, "cartridges", os.path.basename(path)))
            total_cart_bytes += os.path.getsize(path)

        data_img = os.path.join(stage_dir, "data", "data.img")
        subprocess.run(["truncate", "-s", "512M", data_img], check=True)
        subprocess.run(["mkfs.ext4", "-F", "-L", "CARTDATA", data_img], check=True, stdout=subprocess.DEVNULL)

        # 3. Format exFAT partition
        exfat_size_bytes = total_cart_bytes + (512 * 1024 * 1024) + (128 * 1024 * 1024)
        exfat_size_mb = (exfat_size_bytes + 1048575) // (1024 * 1024)
        print(f"[kurogane] -> Formatting exFAT partition ({exfat_size_mb}MB)...")
        subprocess.run(["truncate", "-s", f"{exfat_size_mb}M", exfat_img], check=True)
        subprocess.run(["mkfs.exfat", "-n", "CARTRIDGES", exfat_img], check=True, stdout=subprocess.DEVNULL)

        # 4. Populate exFAT via QEMU micro-populator
        print("[kurogane] -> Populating exFAT volume via micro-kernel...")
        pop_cmd = [
            "qemu-system-x86_64",
            "-enable-kvm", "-cpu", "host",
            "-m", "512M",
            "-no-reboot",
            "-kernel", kernel,
            "-initrd", initrd,
            "-drive", f"file={exfat_img},format=raw,if=virtio",
            "-virtfs", f"local,path={stage_dir},mount_tag=hosthub,security_model=none,readonly=on",
            "-append", "console=ttyS0 quiet cartilage_populate=1",
            "-display", "none",
            "-serial", "stdio"
        ]
        subprocess.run(pop_cmd, check=True, stdout=subprocess.DEVNULL)

        # 5. Assemble GPT Disk Image
        SECTOR_SIZE = 512
        SECTORS_PER_MIB = 2048
        current_sector = 2048

        esp_sectors = os.path.getsize(esp_img) // SECTOR_SIZE
        esp_sectors = ((esp_sectors + SECTORS_PER_MIB - 1) // SECTORS_PER_MIB) * SECTORS_PER_MIB
        esp_start = current_sector
        current_sector += esp_sectors

        exfat_sectors = os.path.getsize(exfat_img) // SECTOR_SIZE
        exfat_sectors = ((exfat_sectors + SECTORS_PER_MIB - 1) // SECTORS_PER_MIB) * SECTORS_PER_MIB
        exfat_start = current_sector
        current_sector += exfat_sectors

        total_sectors = current_sector + 2048
        total_size_bytes = total_sectors * SECTOR_SIZE

        print(f"[kurogane] -> Writing GPT partition table ({total_size_bytes // (1024*1024)} MiB)...")
        subprocess.run(["truncate", "-s", str(total_size_bytes), raw_disk], check=True)

        sfdisk_script = (
            "label: gpt\n"
            f"label-id: {os.urandom(16).hex().upper()[:8]}-0000-4000-8000-000000000002\n\n"
            f"start={esp_start}, size={esp_sectors}, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name=\"CARTBOOT\"\n"
            f"start={exfat_start}, size={exfat_sectors}, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name=\"CARTRIDGES\"\n"
        )
        subprocess.run(["sfdisk", "--no-reread", raw_disk], input=sfdisk_script.encode(), check=True, stdout=subprocess.DEVNULL)

        print("[kurogane] -> Writing partition blocks into raw disk...")
        subprocess.run(["dd", f"if={esp_img}", f"of={raw_disk}", f"seek={esp_start}", "bs=512", "conv=notrunc", "status=none"], check=True)
        subprocess.run(["dd", f"if={exfat_img}", f"of={raw_disk}", f"seek={exfat_start}", "bs=512", "conv=notrunc", "status=none"], check=True)

        os.makedirs(os.path.dirname(os.path.abspath(output_img)), exist_ok=True)
        shutil.move(raw_disk, output_img)
        print(f"[kurogane] Dynamic Hub image composed successfully: {output_img} ({os.path.getsize(output_img) // (1024*1024)} MiB)")
        return output_img

    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)


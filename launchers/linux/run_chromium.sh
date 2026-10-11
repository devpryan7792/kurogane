#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${REPO_ROOT}/build"
CARTRIDGE_IMG="${BUILD_DIR}/cartridge_chromium.img"

if [[ ! -f "${CARTRIDGE_IMG}" ]]; then
    if [[ -f "${BUILD_DIR}/cartridge_chromium_arch.img" ]]; then
        CARTRIDGE_IMG="${BUILD_DIR}/cartridge_chromium_arch.img"
    else
        echo "Error: Chromium cartridge image not found in ${BUILD_DIR}." >&2
        exit 1
    fi
fi

KERNEL=""
INITRD=""
if [[ -f "${BUILD_DIR}/initramfs-linux.img" ]]; then
    INITRD="${BUILD_DIR}/initramfs-linux.img"
elif [[ -f "/var/lib/kurogane/rootfs/boot/initramfs-linux.img" ]]; then
    INITRD="/var/lib/kurogane/rootfs/boot/initramfs-linux.img"
elif [[ -f "/var/lib/cartilage/rootfs/boot/initramfs-linux.img" ]]; then
    INITRD="/var/lib/cartilage/rootfs/boot/initramfs-linux.img"
elif [[ -f "/boot/initramfs-linux.img" ]]; then
    INITRD="/boot/initramfs-linux.img"
fi

if [[ -f "/var/lib/kurogane/rootfs/boot/vmlinuz-linux" ]]; then
    KERNEL="/var/lib/kurogane/rootfs/boot/vmlinuz-linux"
elif [[ -f "/var/lib/cartilage/rootfs/boot/vmlinuz-linux" ]]; then
    KERNEL="/var/lib/cartilage/rootfs/boot/vmlinuz-linux"
elif [[ -f "/boot/vmlinuz-linux" ]]; then
    KERNEL="/boot/vmlinuz-linux"
fi

if [[ -z "${KERNEL}" || -z "${INITRD}" ]]; then
    echo "Error: Kernel or initramfs not found." >&2
    exit 1
fi

if [[ $EUID -eq 0 && -n "${SUDO_USER:-}" ]]; then
    SUDO_UID=$(id -u "${SUDO_USER}")
    export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/${SUDO_UID}}"
fi

KVM_FLAGS=""
if [[ -c /dev/kvm ]]; then
    KVM_FLAGS="-enable-kvm -cpu host"
fi

TARGET_URL="${1:-https://duckduckgo.com}"

echo "============================================================"
echo "Starting Kurogane OS — Chromium Desktop Web Browser"
echo "Runtime: Chromium Ozone Wayland on Cage Compositor"
echo "Start URL: ${TARGET_URL}"
echo "Network: virtio-net with automatic DHCP"
echo "Audio:   Intel HDA Sound Card with ALSA dmix"
echo "Memory:  2048 MB allocated"
echo "Cartridge: ${CARTRIDGE_IMG}"
echo "Kernel:    ${KERNEL}"
echo "Initrd:    ${INITRD}"
echo "Console:   VT2 Debug Console (Ctrl+Alt+F2, passcode: cartilage42)"
echo "============================================================"

exec qemu-system-x86_64 \
    ${KVM_FLAGS} \
    -kernel "${KERNEL}" \
    -initrd "${INITRD}" \
    -drive file="${CARTRIDGE_IMG}",format=raw,if=virtio,readonly=on \
    -vga virtio \
    -netdev user,id=net0 \
    -device virtio-net-pci,netdev=net0 \
    -audiodev id=snd0,driver=none \
    -device intel-hda \
    -device hda-duplex,audiodev=snd0 \
    -append "console=tty1 console=ttyS0 root=/dev/vda rootfstype=erofs init=/init url=${TARGET_URL}" \
    -usb -device usb-tablet \
    -m 2048M \
    -display gtk \
    -serial stdio

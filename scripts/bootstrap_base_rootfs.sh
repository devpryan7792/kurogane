#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR="${REPO_ROOT}/build"
PACMAN_CONF="${BUILD_DIR}/pacman.conf"

DEFAULT_ROOTFS="/var/lib/kurogane/rootfs"
[[ -d "/var/lib/cartilage/rootfs" ]] && DEFAULT_ROOTFS="/var/lib/cartilage/rootfs"
TARGET_DIR="${1:-${DEFAULT_ROOTFS}}"

echo "============================================================"
echo "Kurogane OS — Building Base Rootfs"
echo "Target: ${TARGET_DIR}"
echo "Pacman Config: ${PACMAN_CONF}"
echo "============================================================"

for tool in pacstrap arch-chroot mkfs.erofs qemu-system-x86_64; do
    if ! command -v "$tool" &>/dev/null; then
        echo "Error: missing required tool '$tool'" >&2
        exit 1
    fi
done

mkdir -p "${TARGET_DIR}"

if [[ -f "${TARGET_DIR}/boot/vmlinuz-linux" ]] && [[ "${FORCE_REBUILD:-0}" != "1" ]]; then
    echo "==> Base rootfs already exists at ${TARGET_DIR}. Skipping pacstrap."
    echo "    (Set FORCE_REBUILD=1 to force clean re-bootstrap)."
else
    echo "==> Running pacstrap (base, linux, linux-firmware)..."
    set +o pipefail
    yes | pacstrap -C "${PACMAN_CONF}" -c -K -M "${TARGET_DIR}" base linux linux-firmware
    set -o pipefail
fi

if [[ ! -e "${REPO_ROOT}/rootfs" ]]; then
    ln -s "${TARGET_DIR}" "${REPO_ROOT}/rootfs" 2>/dev/null || true
fi

echo "==> Verifying arch-chroot..."
arch-chroot "${TARGET_DIR}" /bin/true
echo "==> arch-chroot /bin/true passed!"

if [[ -f "${TARGET_DIR}/boot/vmlinuz-linux" ]]; then
    echo "==> Kernel verified: ${TARGET_DIR}/boot/vmlinuz-linux"
else
    echo "Error: ${TARGET_DIR}/boot/vmlinuz-linux not found!" >&2
    exit 1
fi

if [[ -f "${TARGET_DIR}/boot/initramfs-linux.img" ]]; then
    echo "==> Initramfs verified: ${TARGET_DIR}/boot/initramfs-linux.img"
else
    echo "Warning: initramfs-linux.img not found, running mkinitcpio..."
    arch-chroot "${TARGET_DIR}" mkinitcpio -P
fi

# Invalidate cached base_template so subsequent builds don't reuse stale kernel modules
rm -rf /var/lib/kurogane/base_template /var/lib/cartilage/base_template

echo "==> Compiling initial base cartridge: ${BUILD_DIR}/cartridge_base_arch.img..."
mkdir -p "${BUILD_DIR}"
mkfs.erofs --all-root -zlz4hc,12 "${BUILD_DIR}/cartridge_base_arch.img" "${TARGET_DIR}"

echo "==> Base rootfs and cartridge_base_arch.img build completed successfully."

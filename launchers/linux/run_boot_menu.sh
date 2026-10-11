#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${REPO_ROOT}/build"
COMBINED_IMG="${BUILD_DIR}/cartilage_combined.img"

export PATH="${HOME}/.local/bin:${PATH}"

OVMF_BIOS=""
for p in /usr/share/edk2/x64/OVMF.4m.fd /usr/share/edk2/x64/OVMF_CODE.4m.fd /usr/share/ovmf/OVMF.fd /usr/share/edk2-ovmf/x64/OVMF.fd /usr/share/OVMF/OVMF.fd /usr/share/qemu/OVMF.fd /usr/share/edk2/x64/OVMF.fd; do
    if [[ -f "$p" ]]; then
        OVMF_BIOS="$p"
        break
    fi
done

if [[ ! -f "${COMBINED_IMG}" || $(stat -c%s "${COMBINED_IMG}") -eq 0 ]]; then
    echo "==> ${COMBINED_IMG} not found. Building via ./kurogane compose..."
    "${REPO_ROOT}/kurogane" compose recipes/browser-dillo.yaml recipes/editor-mousepad.yaml -o "${COMBINED_IMG}"
fi

if [[ $EUID -eq 0 && -n "${SUDO_USER:-}" ]]; then
    SUDO_UID=$(id -u "${SUDO_USER}")
    export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/${SUDO_UID}}"
fi

KVM_FLAGS=""
if [[ -c /dev/kvm ]]; then
    KVM_FLAGS="-enable-kvm -cpu host"
fi

BIOS_FLAG=""
if [[ -n "${OVMF_BIOS}" ]]; then
    BIOS_FLAG="-bios ${OVMF_BIOS}"
fi

echo "============================================================"
echo "Starting Kurogane OS — Unified Multi-Boot UEFI Menu"
echo "Select Dillo or Mousepad using arrow keys and press Enter."
echo "Image:   ${COMBINED_IMG}"
echo "Firmware: ${OVMF_BIOS:-SeaBIOS fallback}"
echo "============================================================"

exec qemu-system-x86_64 \
    ${KVM_FLAGS} \
    -drive file="${COMBINED_IMG}",format=raw,if=virtio \
    -vga virtio \
    -netdev user,id=net0 \
    -device virtio-net-pci,netdev=net0 \
    -usb -device usb-tablet \
    -m 1024M \
    ${BIOS_FLAG} \
    -display gtk \
    -serial stdio

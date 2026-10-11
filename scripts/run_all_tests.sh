#!/usr/bin/env bash
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "Error: scripts/15_run_all_tests.sh requires root privileges for loop devices, chroot, and disk formatting." >&2
    echo "Please execute with: sudo bash scripts/15_run_all_tests.sh (or 'wsl -d Ubuntu -u root ...')" >&2
    exit 1
fi

echo '============================================================'
echo 'Cartilage OS — Full Regression Test Suite'
echo '============================================================'

TESTS=(
    scripts/01_build_base_rootfs.sh
    scripts/01_test_qemu.sh
    scripts/02_build_cartridge.sh
    scripts/02_test_cartridge_qemu.sh
    scripts/03_test_ephemeral_storage.sh
    scripts/04_test_persistent_storage.sh
    scripts/05_test_host_access.sh
    scripts/06_test_builder_cli.sh
    scripts/07_build_combined_image.sh
    scripts/07_test_boot_menu.sh
    scripts/08_test_debug_console.sh
    scripts/09_run_benchmarks.sh
    scripts/10_test_networking.sh
    scripts/11_test_audio.sh
    scripts/12_test_chromium.sh
    scripts/13_test_flasher.sh
    scripts/14_test_alpine_cartridge.sh
    scripts/15_test_cartilage_cli.sh
)

for test_script in "${TESTS[@]}"; do
    echo '-----------------------------------------------------------'
    echo "[RUNNING] $test_script"
    echo '-----------------------------------------------------------'
    if bash "$test_script"; then
        echo "[PASS] $test_script"
    else
        echo "[FAIL] $test_script failed! Aborting test suite."
        exit 1
    fi
done

echo '============================================================'
echo '[SUCCESS] All tests passed! Benchmark numbers are maintained.'
echo '============================================================='

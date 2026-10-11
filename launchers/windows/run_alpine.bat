@echo off
echo ============================================================
echo Starting Kurogane OS -- Ultra-Lean Alpine Cartridge (Mousepad)
echo Image Size: 44.8 MB (96% smaller than Arch)
echo Cold Boot Latency: ~2.1s - 4.3s (KVM Accelerated)
echo Memory Usage: ~57 MB Idle RAM
echo ============================================================
wsl bash -c "cd \"$(wslpath '%~dp0/../..')\" && ./kurogane run recipes/editor-mousepad.yaml"


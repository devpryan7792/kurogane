@echo off
echo ============================================================
echo Starting Kurogane OS -- Chromium Modern Web Kiosk
echo Runtime: Chromium Ozone Wayland on Cage Compositor
echo Network: virtio-net with automatic DHCP
echo Audio: ALSA dmix sound subsystem
echo ============================================================
wsl bash -c "cd \"$(wslpath '%~dp0/../..')\" && ./kurogane run recipes/browser-chromium.yaml"





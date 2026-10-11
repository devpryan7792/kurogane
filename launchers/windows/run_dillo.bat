@echo off
echo ============================================================
echo Starting Kurogane OS -- Web Browser (Dillo)
echo Acceleration: Hardware KVM enabled
echo Mouse: USB Tablet absolute pointer integration
echo ============================================================
wsl bash -c "cd \"$(wslpath '%~dp0/../..')\" && ./kurogane run recipes/browser-dillo.yaml"

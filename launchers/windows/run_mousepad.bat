@echo off
echo ============================================================
echo Starting Kurogane OS -- Text Editor (Mousepad)
echo Acceleration: Hardware KVM enabled
echo Mouse: USB Tablet absolute pointer integration
echo ============================================================
wsl bash -c "cd \"$(wslpath '%~dp0/../..')\" && ./kurogane run recipes/editor-mousepad.yaml"

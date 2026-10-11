@echo off
echo ============================================================
echo Starting Kurogane OS -- Unified Multi-Boot UEFI Menu
echo Select appliance using arrow keys and press Enter.
echo ============================================================
wsl bash -c "cd \"$(wslpath '%~dp0/../..')\" && ./kurogane run build/cartilage_combined.img"

# Flashing Kurogane to a USB drive — macOS & Windows

This guide shows how to write a Kurogane `.img` disk image (the kind `kurogane compose` and `kurogane build`
produce) to a physical USB drive from **macOS** or **Windows**, using either a GUI tool (Rufus or
Balena Etcher) or `dd`.

> **Linux?** You do not need this guide. The bundled flasher already handles the target-block-device
> write with built-in safety checks (host-root protection, mounted-partition check, internal-drive
> guard) and a `--dry-run` layout preview:
>
> ```bash
> # Preview the GPT layout without writing a single block:
> sudo ./kurogane flash --target /dev/sdX --dry-run recipes/terminal-foot.yaml
>
> # Actually write it (you will be prompted to type 'yes' to confirm):
> sudo ./kurogane flash --target /dev/sdX recipes/terminal-foot.yaml
> ```
>
> See the README's "Flash safely to target block device" section for the full `flash` / `init-hub`
> reference.

## Before you start: build or obtain the image

The flash steps below write a **`.img`** file to the drive. First get one of these:

1. **Compose a multi-boot image locally (Linux host)** — the recommended path:
   ```bash
   ./kurogane compose -o build/kurogane_multiboot.img \
       recipes/terminal-foot.yaml recipes/editor-mousepad.yaml
   # or a single cartridge:
   ./kurogane build recipes/terminal-foot.yaml -o build/cartridge_foot.img
   ```
2. **Use a prebuilt image** you or the project maintainers have published (e.g. a
   `build/kurogane_multiboot.img` from CI or a release artifact).

Keep the `.img` on your Mac or Windows machine before proceeding.

## ⚠️ Choose the correct target drive

Every step in this guide permanently erases **all data on the target USB drive**. Flashing
the image to a wrong device number — or to your internal/boot disk — will destroy it and can
render a machine unbootable.

Before you run any of the commands or pick a drive in a GUI tool, confirm the USB drive's
exact identity:

- **macOS**: `diskutil list` — your USB stick will typically appear as `disk2` / `disk3`
  (the internal drive is `disk0`). Look at the size (e.g. "32 GiB") to confirm.
- **Windows**: `Get-Volume` or `diskpart` → `list disk` (PowerShell, run as administrator).
  USB media will usually be `Disk 1` or higher; your internal SSD/NVMe is `Disk 0`.

Do the following **every time** before flashing:

1. Unmount / eject the USB volume on macOS (`diskutil unmountDisk /dev/diskN`) or
   safely remove it from File Explorer on Windows, so no partition is actively in use.
2. Back up anything on the USB stick you care about.
3. Double-check the size of the device you are about to write matches the USB stick, not
   your internal disk.

---

## macOS

### Option A — Balena Etcher (GUI, simplest)

1. Install Balena Etcher: `brew install --cask etcher` (or download from balena.io/etcher).
2. Launch Etcher. Under **Flash & Unlock**, choose **File**: select your `.img`.
3. Choose **Target**: select the USB stick (confirm the size label matches the drive you
   inspected with `diskutil list`).
4. Click **Flash**. Etcher will erase and write the image. Wait for the success banner,
   then **eject the drive** before unplugging.

### Option B — `dd` (terminal)

1. Identify the raw disk device (NOT a partition):
   ```bash
   diskutil list
   # e.g. /dev/disk2  (32 GiB)   ← your USB stick
   ```
2. Unmount it:
   ```bash
   diskutil unmountDisk /dev/disk2
   ```
3. Write the image to the **raw disk** device (`diskN`, never `diskNs`):
   ```bash
   sudo dd if=build/kurogane_multiboot.img of=/dev/disk2 bs=1m
   ```
4. `dd` will show a transfer-rate summary when done. Eject:
   ```bash
   diskutil eject /dev/disk2
   ```

> `dd` on macOS writes block-by-block with no progress bar; add `| pv` if you installed
> `pv` (Homebrew) to see a progress meter:
> `sudo dd if=... of=/dev/disk2 bs=1m | pv -s $(stat -f %z build/kurogane_multiboot.img) > /dev/null`

---

## Windows

### Option A — Rufus (GUI)

1. Download Rufus (rufus.ie) — the portable `.exe` does not need installation.
2. Plug in the USB stick. In Rufus, **Device**: pick the correct drive (verify the size
   matches your USB stick; do **not** pick the internal disk).
3. Under **Boot selection**, choose **Disk or ISO image (please select)** and browse to
   your `.img` file.
4. **Partition scheme**: keep `GPT` (Kurogane images use a GPT layout with an ESP boot
   partition). **Target system**: `UEFI`.
5. Click **START**. Rufus will warn about writing the disk; confirm the drive one last
   time and proceed. When done, click **CLOSE** and safely remove the drive.

> Rufus may display a warning that this is not a Microsoft UEFI image — that is expected.
> The image is a full GPT + ESP + EROFS rootfs, not a Windows install ISO.

### Option B — Balena Etcher (GUI)

Same steps as macOS Balena Etcher: **Flash & Unlock → File → Target (verify size) → Flash**.

### Option C — no native `dd` on Windows

Windows ships no `dd`-equivalent for raw block writes, so the raw-write path on this OS goes
through a GUI tool: use **Rufus** (Option A) or **Balena Etcher** (Option B) above. Both
write the `.img` byte-for-byte to the selected drive, which is exactly what `dd` would do.

If you genuinely need a scripted raw write (e.g. in CI or an image pipeline), the cleanest
option is to do the write on a Linux host with `dd` (the macOS `dd` section works identically
there), or use a small cross-platform helper that opens the drive by handle. There is no
first-party Windows command that matches `dd`'s "write these bytes to this block device",
so a hand-rolled PowerShell `Write-Binary` / hex-dump pipeline is not recommended for this
task.

---

## After flashing

- Eject / safely-remove the USB drive.
- Plug it into the target machine and set the firmware to boot from USB.
- Kurogane boots its `kurogane-init` PID-1 micro-supervisor from the ESP into the EROFS
  rootfs; the session comes up in kiosk mode per the recipe.
- If you composed a **Dynamic Hub** image (`kurogane init-hub`, Mode 2), the hub's exFAT
  `CARTRIDGES` partition accepts additional `*.img` cartridges you copy in by hand —
  the ESP's menu will list every cartridge found at boot.

## Troubleshooting

- **Drive not showing up in the GUI** — unplug and replug; some hubs do not expose the
  media correctly. Try a different USB port or cable.
- **Rufus/`dd` succeeds but the machine does not boot from the USB** — check the firmware's
  boot menu (often `F11`/`F12`/`Esc` at power-on) and ensure **UEFI** (not Legacy/CSM)
  boot is selected; Kurogane ESPs are UEFI bootloaders (`systemd-boot`), not MBR/Legacy.
- **"Volume is in use" during `dd`** — eject / unmount the USB volume first, then retry.
- **Went to the wrong drive** — stop immediately. On macOS, `diskutil list` to verify
  before re-trying; on Windows, `Get-Volume` in PowerShell. The flasher (`./kurogane
  flash`) on Linux refuses the host root device and internal fixed drives by default,
  so the manual `dd` / Rufus path is the one place you must be careful yourself.

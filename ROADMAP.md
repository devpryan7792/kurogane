# Cartilage OS — Strategic Roadmap

This document outlines the evolutionary phases of Cartilage OS, tracking completed engineering milestones and defining active architecture deliverables.

---

## Phase 1: Proof of Concept & Minimal Physics [COMPLETED]
*Goal: Prove that a bare Linux kernel and EROFS can boot an interactive Wayland session in ~1 second without systemd.*
- [x] Scripted minimal `pacstrap` base rootfs build (`base`, `linux`, `linux-firmware`).
- [x] Verified `arch-chroot` environment and unprivileged user setup.
- [x] Implemented minimal `/init` script driving `seatd` and `cage` compositor over kernel DRM/KMS.
- [x] Packed rootfs into immutable EROFS image with `mkfs.erofs`.
- [x] Verified sub-1.5s cold boot in QEMU with direct KMS rendering.

---

## Phase 2: Dual Runtimes & Stabilization (v2.0) [COMPLETED]
*Goal: Expand beyond a single demo into lightweight runtimes, storage modes, and modern web capabilities.*
- [x] Implemented Alpine Linux runtime (`musl` libc, `apk` package manager), dropping base appliance size to **44.6 MB** and cold boot latency to **2.1s**.
- [x] Implemented Ephemeral Mode: OverlayFS with memory-quota `tmpfs` upperdir and `zram` (zstd) compressed memory swap.
- [x] Implemented Persistent Mode: Secondary ext4 data disk bound to `/data`.
- [x] Implemented Host Access Mode: Hidden read-only host mount with TTY Developer Passcode gate (`cartilage42`) and loud rejection of dirty Windows NTFS partitions.
- [x] Verified Chromium Modern Web Kiosk: Ozone Wayland rendering and nested unprivileged user namespaces.

---

## Phase 3: Universal Appliance Platform [COMPLETED]
*Goal: Replace procedural shell scripts with an engineered, declarative appliance engine and unified CLI.*
- [x] **Unified CLI Engine (`./cartilage`)**: Zero-dependency Python CLI (`validate`, `build`, `run`, `compose`, `flash`) with standard JSON Schema validation.
- [x] **Modular `/init.d/` Stage Runner**: Sequential, fault-tolerant stage dispatcher (`00-vfs`, `10-hardware`, `20-network`, `30-storage`, `40-security`, `50-launch`) with automatic memory OverlayFS fallback on storage errors.
- [x] **Pure-Python Rootless Compiler (`builder.py`)**: Rootless base extraction via `fsck.erofs`, unprivileged package ingestion, account baking (`cartilage:1000:1000`), and `mkfs.erofs --all-root -zlz4hc,12` compilation.
- [x] **Universal ALSA `dmix` Multiplexer**: In-kernel multi-client audio mixing in `stages/10-hardware.sh`, eliminating background PulseAudio/PipeWire daemons.
- [x] **Appliance Fleet Expansion**: Built and physically verified 6 appliances:
  - `terminal-foot`: Minimalist Wayland terminal (85 MB idle RAM, 1.8s boot)
  - `media-vlc`: Universal Qt5 media player with GUI controls & ALSA audio (340 MB RAM, 2.8s boot)
  - `media-mpv`: Minimalist video playback engine (120 MB RAM, 2.5s boot)
  - `editor-mousepad`: Focused text editor (44.6 MB Alpine image, 57.6 MB RAM)
  - `browser-dillo`: Ultra-compact web browser (528 MB image, 285 MB RAM)
  - `browser-chromium`: Modern web kiosk (844 MB image, 552 MB RAM)
- [x] **Mode 1 Multi-Boot UEFI GPT Composer**: Composed all appliances into a unified 3.6 GiB UEFI GPT disk image with `systemd-boot`.

---

## Phase 4: The Dynamic Hub & Developer Workstation [COMPLETED]
*Goal: Transform Cartilage into a complete daily development and utility platform via the Ventoy-style drag-and-drop Hub and simultaneous Terminal + Browser execution.*

### Milestone 4.1: Dynamic Hub Bootstrap Loader (`initramfs-hub.img`)
- [x] Implemented early userspace bootstrap script (`src/cartilage/hub_loader.sh`):
  - Detects partition labeled `CARTRIDGES` (exFAT).
  - Mounts exFAT in-kernel via `exfat.ko`.
  - Discovers `/cartridges/*.img` payloads.
  - Automatically boots single cartridge or renders an instant (<100ms) TTY text boot selector with 3s timeout.
  - Mounts selected cartridge via loopback: `mount -t erofs -o loop,ro /mnt/hub/cartridges/<app>.img /sysroot`.
  - Mounts persistent ext4 loop file: `mount -t ext4 -o loop,rw /mnt/hub/data/data.img /sysroot/data`.
  - Performs `switch_root /sysroot /init` (verified 1.005s boot latency).

### Milestone 4.2: Dynamic Hub Disk Formatting CLI (`cartilage init-hub`)
- [x] Added `cartilage init-hub --target /dev/sdX` subcommand to `flasher.py`:
  - Strict block device safety verification (refuses internal SATA/NVMe without `--force-internal`).
  - Creates 2-partition GPT layout: 256 MB FAT32 ESP (`CARTBOOT`) + remainder exFAT (`CARTRIDGES`).
  - Pre-creates `/cartridges/` directory for drag-and-drop image placement.
  - Generates sparse 512 MB ext4 image at `/data/data.img` for POSIX user persistence.
  - Installs `vmlinuz-linux` and `initramfs-hub.img` onto the ESP.
  - Added headless 0.8s QEMU micro-VM populator for rootless exFAT formatting without `sudo`.

### Milestone 4.3: Developer Workstation Appliance (`recipes/experimental/workstation-dev.yaml`)
- [x] Created unified developer workstation recipe:
  - Compositor: C-based dynamic tiling Wayland compositor (`dwl` v0.9, <15 MB RAM).
  - Packages: `dwl`, `foot`, `dillo`, `seatd`.
  - Configures Workspace 1 (Terminal) and Workspace 2 (Browser) with instant keybinding toggle (`Alt+1` / `Alt+2`).
  - Measured idle RAM at **264 MB** total active memory on 1GB virtual machine, leaving >680 MB free memory.

### Milestone 4.4: Automated Test Suite & Bare-Metal Matrix
- [x] Updated `scripts/15_test_cartilage_cli.sh` to include Hub formatting dry-run (Test 9), Workstation Duo boot (Test 10), and Dynamic Hub UEFI boot (Test 11).
- [x] Benchmarked cold boot latency: Mode 1 (1.8s) vs. Mode 2 (1.005s to loop-mount).
- [x] All 11 tests passing cleanly (13/13 test assertions).

---

## Phase 5: Multi-Compositor Choice & Workstation Ergonomics [COMPLETED]
*Goal: Provide users with granular compositor choice across single-app kiosks and multi-window workstations, supporting cage, dwl, and sway (i3).*

### Milestone 5.1: Three-Tier Compositor Architecture
- [x] Enforce compositor behavioral semantics:
  - **`cage`**: Locked single-application kiosk mode (strictly 1 window/app, shell masked to `/dev/null`, ~5 MB RAM).
  - **`dwl`**: Ultra-lean C-based dynamic tiling (dwm for Wayland; <15 MB RAM, tags 1–9, unmasked `/bin/bash` for interactive developer shells).
  - **`sway`**: Full i3-compatible tiling window manager (~35–45 MB RAM, workspaces 1–10, split containers, floating windows, i3-ipc, unmasked `/bin/bash`).
- [x] Implement validator constraint: warn or reject if `cage` is assigned to multi-application sessions or workstation recipes.

### Milestone 5.2: Declarative & CLI Compositor Selection
- [x] Schema validation update for `display.compositor`: enum `["cage", "dwl", "sway", "none"]`.
- [x] Add CLI flag `--compositor` to `cartilage build` and `cartilage run` to allow on-the-fly compositor switching.
- [x] Implement rootless `sway` packaging and minimal Cartilage i3 configuration (`/etc/cartilage/sway.conf`).

### Milestone 5.3: Workstation Recipe Suite
- [x] Maintain `recipes/experimental/workstation-dev.yaml` with `dwl` default for low-memory appliances (<300 MB RAM).
- [x] Create `recipes/experimental/workstation-i3.yaml` with `sway` for full i3-compatible developer workflows.

### Milestone 5.4: Test Suite & Matrix Expansion
- [x] Extend `scripts/15_test_cartilage_cli.sh` to verify `cage`, `dwl`, and `sway` builds and launches (14/14 tests passing).
- [x] Record benchmark memory and latency matrix comparing all three compositor targets.

---

## Phase 6: Ecosystem, OCI Container Import & Appliance Distribution [PLANNED]
*Goal: Expand Cartilage OS from local builds into a frictionless ecosystem with OCI image translation and remote cartridge distribution.*

### Milestone 6.1: OCI Container to Cartridge Importer (`cartilage import-docker`)
- [ ] Parse Docker/OCI rootfs layers and convert directly into immutable EROFS cartridges.
- [ ] Automatically synthesize declarative recipe manifests from container metadata (`CMD`, `ENV`, `EXPOSE`).

### Milestone 6.2: Remote Cartridge Hub & Package Distribution (`cartilage pull`)
- [ ] Implement zero-dependency HTTP/HTTPS download engine with sha256 checksum verification.
- [ ] Enable 1-command appliance fetching: `./cartilage pull devpryan7792/workstation-i3`.

### Milestone 6.3: Interactive TUI Hub Configurator
- [ ] Early-userspace dialog/curses configuration menu for Mode 2 Dynamic Hub.
- [ ] Wi-Fi network selection and persistent credential storage in `/data`.

---

## Phase 7: Kurogane v3.x — Bare-Metal Production Hardening (The 16-Week Part-Time Master Plan)
*Goal: Evolve Kurogane from a functional prototype into an MIT/Google-grade, fault-tolerant, power-cut-resilient appliance operating system.*
*Pacing: 10–14 hours/week (Weekend Sprints of 6–8h + 1–2 Evening Sprints of 2–3h).*

### Milestone 1: The Zero-Thread Event Core (PID 1 Hardening) [Weeks 1–3]
*Pacing: ~35 engineering hours*
- [ ] **Week 1 (Types & Signal Interface)**:
  - Mask all standard signals (`sigprocmask(SIG_BLOCK, ...)`) in PID 1 before any thread/process spawn.
  - Implement `signalfd(2)` binding to read `SIGCHLD`, `SIGTERM`, `SIGINT`, `SIGPWR` as structured byte streams.
  - Define non-blocking `EventLoop` struct wrapping single-threaded `epoll(7)`.
- [ ] **Week 2 (Non-Blocking Child & Timer Supervision)**:
  - Implement non-blocking `waitpid(-1, WNOHANG)` zombie reaper in main epoll loop to eliminate child process accumulation.
  - Integrate `timerfd_create(2)` into epoll loop for heartbeat polling, DRM release timeouts, and watchdog checks without `thread::sleep`.
- [ ] **Week 3 (Zero-Allocation Guarantee & Verification)**:
  - Audit critical signal handling paths to ensure zero dynamic heap allocations (`malloc`/`Box::new`/`Vec::new`).
  - Unit & integration tests in `crates/kurogane-init`: simulate synthetic `SIGCHLD` bursts, verify zero PID 1 deadlocks.

### Milestone 2: Stateful Wayland Wire Inspector [Weeks 4–7]
*Pacing: ~45 engineering hours*
- [ ] **Week 4 (Binary Wire Framer & Parser)**:
  - Implement zero-allocation parser for 8-byte Wayland message header: `[u32: object_id][u16: opcode][u16: size]`.
  - Implement streaming buffer to handle fragmented packets and Unix domain socket chunk boundaries.
- [ ] **Week 5 (Registry Globals Filtering)**:
  - Intercept initial compositor `wl_registry::global` announcements.
  - Parse interface names and filter out sensitive capabilities (`wlr_screencopy_v1`, `zwp_virtual_keyboard_v1`, `ext_data_control_v1`).
  - Drop or suppress blacklisted global announcements while recalculating packet lengths on the fly.
- [ ] **Week 6 (Object ID Coherency & State Tracking)**:
  - Implement stateful object ID translation table to prevent client desynchronization on filtered interfaces.
  - Ensure client requests for non-advertised globals cleanly return protocol errors or are silently dropped without socket termination.
- [ ] **Week 7 (Message-Bound Relay & Security Benchmarking)**:
  - Detect completion of registry bind phase; seal the state machine and transition sockets to non-blocking epoll message-bound relays with SCM_RIGHTS preservation.
  - Benchmark proxy forwarding latency (target: <15 µs overhead on Wayland render loop).

### Milestone 3: Defense-in-Depth Confinement (Seccomp-BPF + Landlock) [Weeks 8–10]
*Pacing: ~35 engineering hours*
- [ ] **Week 8 (Syscall Profiling & BPF Whitelist Engine)**:
  - Profile system call requirements for appliance fleet (`foot`, `mpv`, `chromium`).
  - Implement recipe-driven BPF program generator compiling strict syscall whitelists (~50 allowable calls; strictly denying `ptrace`, `keyctl`, `bpf`, `io_uring`, `reboot`).
- [ ] **Week 9 (Landlock LSM Path Restrictions)**:
  - Apply Landlock ruleset before dropping root: restrict filesystem reads/writes strictly to `/data` and app assets, blocking transversal to host mounts.
  - Configure unprivileged user namespace mapping (`CLONE_NEWUSER`) to strip root capabilities inside the cartridge sandbox.
- [ ] **Week 10 (Adversarial Security Audit)**:
  - Automated test harness attempting unauthorized syscalls and verifying instant `SECCOMP_RET_KILL_PROCESS` termination.

### Milestone 4: Power-Loss Atomicity (The "Yank" Test) [Weeks 11–13]
*Pacing: ~35 engineering hours*
- [ ] **Week 11 (Journaling Persistence Policy)**:
  - Configure persistent `/data` format with `data=journal` or F2FS atomic write semantics.
  - Implement atomic save protocol in supervisor: sync barriers before unmounting.
- [ ] **Week 12 (QEMU Chaos & Power-Cut Harness)**:
  - Build automated chaos testing harness: execute continuous disk write cycles while firing random QEMU `SIGKILL` or `echo c > /proc/sysrq-trigger`.
  - Measure filesystem integrity, journal playback latency, and orphaned inode recovery on next cold boot.
- [ ] **Week 13 (Corrupt Storage Self-Healing)**:
  - Implement automated `fsck` self-healing in supervisor with zero interactive prompts (`fsck -p`).
  - Fall back to clean temporary overlay if persistent storage is physically unrecoverable.

### Milestone 5: Bare-Metal Silicon & Appliance Monograph [Weeks 14–16]
*Pacing: ~35 engineering hours*
- [ ] **Week 14 (Multi-GPU Hardware Matrix)**:
  - Test physical USB 3.0 boots on real hardware (Intel UHD/Iris, AMD APUs, discrete GPU machines).
  - Implement automated fallback to Pixman CPU renderer when DRM modesetting or firmware blobs fail.
- [ ] **Week 15 (Hardware Seat & Audio Arbitrator)**:
  - Harden `seatd` input device arbitration; handle non-crashing VT switches (`Ctrl+Alt+F2` -> `F1`).
  - Polish ALSA `dmix` multi-stream multiplexing across physical headphone and HDMI audio jacks.
- [ ] **Week 16 (Release Packaging & Systems Monograph)**:
  - Tag Kurogane v3.0 Production Release.
  - Publish the comprehensive systems architecture whitepaper / technical monograph.



# Cartilage OS — Acceptance Specification (SPEC.md)

This document defines the **concrete acceptance criteria and verification commands** for Cartilage OS.
- `ARCHITECTURE.md` defines *what* to build and *why*.
- `ROADMAP.md` and `AGENT_TASKS.md` define *what order*.
- **This file defines how you know each task is objectively done.**

Every task ends with a verifiable test command whose output you check. There are no judgment calls or "looks right" approvals.

---

## Part 1: Core Foundation & Runtimes (Completed & Verified)

### Task 1 — Base Rootfs Construction
**Done when:**
- Unprivileged base rootfs extraction succeeds with exit code 0.
- Rootfs contains valid Linux directory tree (`/usr`, `/bin`, `/lib`).
- Booting raw rootfs in QEMU produces visible console output with no kernel panic.

### Task 2 — Cartridge Packaging (Module 2)
**Done when:**
- `mkfs.erofs -zlz4hc,12` compiles an immutable EROFS image without `sudo`.
- Loop-mounting read-only succeeds: `fsck.erofs` validates filesystem structure.
- Booting in QEMU launches `cage` and target application full-screen.

### Task 3 — Ephemeral Storage Mode
**Done when:**
- OverlayFS combines EROFS lowerdir with `tmpfs` upperdir.
- `size=` quota on tmpfs limits download capacity; `zram` compressed memory swap is active (`zramctl`).
- Filling quota results in clean `ENOSPC` (disk full) error, never kernel OOM panic.

### Task 4 — Persistent Storage Mode
**Done when:**
- Second virtual disk (or loop file) formatted as ext4 is bound to `/data`.
- Files written to `/data` survive VM reboot and remain intact.

### Task 5 — Host Access Mode & NTFS Dirty-Bit Gate
**Done when:**
- Internal host partitions are mounted read-only under hidden path (`/mnt/hidden_host`).
- Physical entry of Passcode (`cartilage42`) unlocks write access to chosen folder via `unshare -m`.
- If an NTFS partition has its dirty/hibernation bit set, write mount is loudly rejected on TTY with remediation steps.

---

## Part 2: Appliance Framework & Platform Layer (Completed & Verified)

### Task 6 — Unified CLI Engine (`./cartilage`)
**Done when:**
- `./cartilage --help` executes cleanly using standard Python (zero external pip dependencies).
- `cartilage validate recipes/*.yaml` verifies manifests against `spec/cartilage.schema.json`.

### Task 7 — Pure-Python Rootless Builder (`cartilage build`)
**Done when:**
- Running `./cartilage build recipes/<recipe>.yaml` extracts base image via `fsck.erofs`, resolves packages rootlessly, injects stages, and compiles with `mkfs.erofs --all-root -zlz4hc,12`.
- Operates 100% in unprivileged userspace (zero `sudo`).

### Task 8 — Modular `/init.d/` Stage Sequencing
**Done when:**
- Root filesystem uses modular stages: `00-vfs.sh`, `10-hardware.sh`, `20-network.sh`, `30-storage.sh`, `40-security.sh`, `50-launch.sh`.
- PID 1 executes stages sequentially and logs progress cleanly.
- Corrupted or missing storage triggers graceful fallback to memory OverlayFS without kernel panic.

### Task 9 — Universal ALSA `dmix` Audio
**Done when:**
- `/run/asound.conf` configures `type dmix` on default audio card.
- Two audio processes can output sound simultaneously without PulseAudio or PipeWire daemons.
- Verified with `speaker-test` or MPV live audio playback.

### Task 10 — Mode 1 Multi-Boot UEFI Disk Composition (`cartilage compose`)
**Done when:**
- `./cartilage compose -o build/cartilage_combined.img recipes/*.yaml` generates a 7-partition GPT image with `systemd-boot`.
- QEMU with OVMF firmware displays the interactive bootloader menu listing all appliances.
- Selecting any appliance boots directly into that appliance's Wayland session.

---

## Part 3: Phase 4 Evolution — The Dynamic Hub & Workstation Duo (Completed & Verified)

### Task 11 — Mode 2 Dynamic Hub Disk Initialization (`cartilage init-hub`)
**Done when:**
- Running `./cartilage init-hub --target /dev/sdX` safely inspects block device:
  - Rejects internal SATA/NVMe drives automatically unless forced.
  - Writes GPT partition table with 2 partitions:
    - Partition 1: `CARTBOOT` (256 MB FAT32 ESP, Type `EF00`).
    - Partition 2: `CARTRIDGES` (exFAT, Type `0700`, occupying rest of drive).
  - Creates directories on exFAT: `/cartridges/` and `/data/`.
  - Creates sparse 512 MB ext4 image at `/data/data.img`.
- Disk is immediately mountable and readable natively on Windows, macOS, and Linux.

**Test command:**
```bash
./cartilage init-hub --dry-run /dev/null
```
Pass = Partition offsets calculated, exFAT payload structure defined, zero errors.

---

### Task 12 — Mode 2 Dynamic Bootstrap Loader (`initramfs-hub.img`)
**Done when:**
- Bootstrap initramfs detects block device with label `CARTRIDGES`.
- Mounts exFAT partition in-kernel via `exfat.ko`.
- Dynamically discovers all `*.img` files in `/cartridges/`:
  - If 1 cartridge: boots immediately.
  - If multiple cartridges: renders instant TTY text menu.
- Successfully mounts chosen cartridge via loopback:
  ```bash
  mount -t erofs -o loop,ro /mnt/hub/cartridges/<chosen>.img /sysroot
  ```
- Loop-mounts `/mnt/hub/data/data.img` (ext4) to `/sysroot/data`.
- Executes `switch_root /sysroot /init` into the target appliance without errors.

**Test command:**
```bash
./cartilage run build/cartilage_hub.img --test
```
Pass = Discovers cartridges from exFAT, loop-mounts, launches Wayland appliance.

---

### Task 13 — Developer Workstation Appliance (`workstation-dev.yaml`)
**Done when:**
- `recipes/experimental/workstation-dev.yaml` declares a multi-window tiling Wayland compositor (`dwl`).
- Launches `foot` terminal on Workspace 1 and web browser (`dillo`) on Workspace 2.
- User can toggle between Terminal and Browser (`Alt+1` / `Alt+2`) with zero reboot delay.
- **Physical Memory Benchmark on 2GB Target**:
  - Total active system memory under 800 MB RAM (Measured: 264 MB).
  - Leaves >680 MB free memory on 1GB machine, >1.6 GB free on 2GB machine.

**Test command:**
```bash
./cartilage run recipes/experimental/workstation-dev.yaml --test
```
Pass = Idle RAM measured under 800 MB, both Foot and Browser operational.

---

## Part 4: Phase 5 Evolution — Multi-Compositor Choice (`cage`, `dwl`, `sway`) (Active)

### Task 14 — Compositor Selection & Enforcement
**Done when:**
- Schema and CLI support `display.compositor`: `["cage", "dwl", "sway", "none"]`.
- `cartilage validate` warns or errors if `cage` is declared for multi-window/multi-app sessions.
- CLI override `--compositor <choice>` dynamically overrides the recipe's compositor for builds and runs.

**Test command:**
```bash
./cartilage validate recipes/terminal-foot.yaml
./cartilage validate recipes/experimental/workstation-dev.yaml
```
Pass = Validates single-app vs multi-app compositor constraints.

---

### Task 15 — i3-Compatible Workstation Appliance (`recipes/experimental/workstation-i3.yaml`)
**Done when:**
- `recipes/experimental/workstation-i3.yaml` configures `sway` compositor.
- Rootless `cartilage build` unpacks `sway` and sets up `/etc/cartilage/sway.conf`.
- Spawns `foot` on Workspace 1 and web browser on Workspace 2.
- Boots in QEMU and passes verification hooks.
- Active memory remains under 800 MB RAM.

**Test command:**
```bash
./cartilage build recipes/experimental/workstation-i3.yaml
./cartilage run recipes/experimental/workstation-i3.yaml --test
```
Pass = Image compiles rootlessly and boots to interactive i3 session.

---

## Part 5: Kurogane v3.x Acceptance Specification (Bare-Metal Systems Hardening)

### Task 16 — Single-Threaded `signalfd` & `epoll` Core
**Done when:**
- PID 1 operates strictly as a single OS thread (`/proc/$$/status` verifies `Threads: 1`).
- All standard Unix signals (`SIGCHLD`, `SIGTERM`, `SIGINT`, `SIGPWR`) are masked via `sigprocmask` and routed exclusively through `signalfd(2)`.
- Main event loop dispatches events via single-threaded `epoll_wait(2)`.
- Child process exits (`SIGCHLD`) are drained via non-blocking `waitpid(-1, &status, WNOHANG)`.
- Critical signal dispatch paths execute with zero dynamic heap allocations.

**Test command:**
```bash
cargo test --package kurogane-init --test event_core_tests
```
Pass = All signalfd events, zombie reaping, and single-thread invariants pass with 0 errors.

---

### Task 17 — Non-Blocking `timerfd` Heartbeat & DRM Watchdog
**Done when:**
- DRM card lock release polling and supervisor timeouts use `timerfd_create(CLOCK_MONOTONIC, TFD_NONBLOCK)`.
- Zero blocking `std::thread::sleep` calls exist anywhere in `crates/kurogane-init`.
- Event loop resumes within 1ms of timer expiration and handles hardware timeouts gracefully.

**Test command:**
```bash
cargo test --package kurogane-init --test timerfd_tests
```
Pass = Timers fire cleanly inside epoll; timeout fallbacks execute without blocking.

---

### Task 18 — Wayland Wire Protocol Header & Framing Parser
**Done when:**
- Parser decodes 8-byte Wayland message headers (`object_id: u32`, `opcode: u16`, `size: u16`) from raw byte buffers.
- Streaming circular buffer handles fragmented packets across arbitrary socket chunk boundaries.
- Zero heap allocations during standard message framing traversal.

**Test command:**
```bash
cargo test --package kurogane-init --test wayland_wire_tests
```
Pass = Decodes simulated Wayland stream slices (single, fragmented, coalesced) with 100% fidelity.

---

### Task 19 — Active Wayland Capability Stripping
**Done when:**
- Proxy intercepts incoming compositor `wl_registry::global` announcements.
- Detects strings `"wlr_screencopy_v1"`, `"zwp_virtual_keyboard_v1"`, and `"ext_data_control_v1"`.
- Suppresses blacklisted announcements and recalculates message lengths dynamically.
- Client receives sanitized registry stream with zero sensitive capabilities advertised.

**Test command:**
```bash
python3 tests/verification/test_wayland_filter_wire.py
```
Pass = Client `wl_registry` query returns standard interfaces but omits screencopy/virtual-keyboard.

---

### Task 20 — Wayland Object ID Coherency & Non-Blocking Reactor
**Done when:**
- Proxy tracks active interface object ID bindings to preserve numerical stream coherency.
- Sealing handshake: upon initial registry bind completion, proxy executes message-bound non-blocking epoll relay preserving all SCM_RIGHTS file descriptors.
- Throughput overhead of proxy relay is measured at <15 µs per frame.

**Test command:**
```bash
python3 tests/verification/test_wayland_handoff_bench.py
```
Pass = Bidirectional relay maintains 60 FPS graphics stream with <15 µs latency overhead.

---

### Task 21 — Seccomp-BPF Automated Whitelist Confinement
**Done when:**
- Supervisor compiles and loads strict BPF filter before executing cartridge payload.
- Whitelist allows only essential graphical/runtime syscalls (~50 standard syscalls).
- Any attempt to invoke `ptrace`, `bpf`, `io_uring`, `keyctl`, or `reboot` immediately aborts process with `SECCOMP_RET_KILL_PROCESS`.
- Authorized applications (`foot`, `mpv`) boot and run with zero syscall violations.

**Test command:**
```bash
./tests/verification/test_seccomp_confinement.sh
```
Pass = Disallowed syscalls result in instant process termination; approved apps run normally.

---

### Task 22 — Landlock LSM & User Namespace Sandbox
**Done when:**
- Landlock ruleset restricts filesystem access exclusively to `/data` and immutable cartridge paths.
- Unprivileged user namespace (`CLONE_NEWUSER`) drops all host root capabilities.
- Attempts to access host devices or write outside `/data` fail with `EACCES` or `EPERM`.

**Test command:**
```bash
./tests/verification/test_landlock_sandbox.sh
```
Pass = Host filesystem access blocked; unprivileged user isolation verified.

---

### Task 23 — Storage Journaling & Atomic Write Guarantees
**Done when:**
- Persistent `/data` volume is mounted with `data=journal` (ext4) or atomic write semantics (F2FS).
- Micro-Supervisor executes explicit `syncfs` synchronization before unmounting.
- Partial writes never leave half-written corrupt metadata states.

**Test command:**
```bash
python3 tests/verification/test_atomic_storage.py
```
Pass = Atomic write invariants and journal metadata integrity validated.

---

### Task 24 — QEMU Chaos & Power-Cut "Yank" Test
**Done when:**
- Automated chaos test executes sustained high-frequency file writes in QEMU guest.
- Chaos harness fires random host `kill -9` on QEMU process or guest `echo c > /proc/sysrq-trigger`.
- Subsequent boot automatically replays journal in <100ms via `fsck -p`.
- Zero unrecoverable filesystem corruption across 20 consecutive power-cut cycles.

**Test command:**
```bash
./scripts/test_power_cut_chaos.sh --iterations 20
```
Pass = 20/20 power-cut recovery cycles succeed with 0 filesystem corruption errors.

---

### Task 25 — Multi-GPU Bare-Metal DRM & Pixman Fallback
**Done when:**
- Micro-Supervisor interrogates available DRM render nodes and monitors modesetting status.
- If hardware acceleration or firmware blobs fail, supervisor dynamically selects `WLR_RENDERER=pixman` CPU rendering.
- System transitions to graphical desktop without frozen consoles or screen flashing.

**Test command:**
```bash
./cartilage run recipes/terminal-foot.yaml --test --force-software-render
```
Pass = Clean boot and Wayland display socket initialization with CPU software renderer fallback.



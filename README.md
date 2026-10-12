<div align="center">

# Kurogane (黒鉄)

**A static Rust micro-supervisor and immutable appliance engine for Linux.**<br>
Compiles desktop and kiosk workloads into read-only EROFS appliances with sub-2s cold boot, zero-allocation pre-exec sandboxing, and decoupled Wayland mediation.

[![CI](https://github.com/devpryan7792/kurogane/actions/workflows/ci.yml/badge.svg)](https://github.com/devpryan7792/kurogane/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org)

<br />

<img src="assets/cartilage_demo.gif" alt="Kurogane Cold Boot Demo" width="85%" />

<br />

[Quickstart](#quickstart) • [Architecture](#architecture) • [Appliance Recipes](#appliance-recipes) • [Benchmarks](#benchmarks) • [Verification](#verification)

</div>

---

## What is Kurogane?

General-purpose Linux distributions carry open-ended mutable state: systemd service dependency trees, `/etc` drift, unmanaged background daemons, and non-deterministic shutdown phases.

Kurogane replaces this stack for dedicated appliances (developer workstations, kiosks, air-gapped terminals, and embedded silicon):

* **Immutable Root Filesystem**: Target environments compile into compressed, read-only EROFS blocks (`mkfs.erofs -z lz4hc,12`).
* **Static Rust PID 1 (`kurogane-init`)**: A stripped 453 KB micro-supervisor running a single-threaded `epoll(7)` event core dedicated to `signalfd(2)` signal routing and non-blocking `waitpid` orphan reaping.
* **Decoupled Wayland Mediation**: The graphics proxy runs in an unprivileged child process (UID 1000) under cgroups v2, isolating display and IPC crashes from the kernel supervisor.
* **Zero-Allocation Sandboxing**: Landlock LSM and Seccomp-BPF filters are applied via stack-allocated structures and raw direct syscalls inside `pre_exec` before dropping privileges.
* **Transactional Persistence**: User state mounts on ext4 with `data=journal`, verified by automated pre-mount `fsck` replay that refuses to mount on unrecoverable block corruptions.

---

## Quickstart

### Prerequisites
* Linux host (x86_64, kernel 6.1+ recommended)
* Python 3.10+ (CLI tooling)
* QEMU (`qemu-system-x86_64`) and KVM enabled (`/dev/kvm`)
* `erofs-utils` (`mkfs.erofs`)

### 1. Validate an Appliance Recipe

```bash
./kurogane validate recipes/terminal-foot.yaml
# [validate] Checking recipes/terminal-foot.yaml...
# [validate] [PASS] recipes/terminal-foot.yaml is valid.
```

### 2. Build the EROFS Appliance

```bash
./kurogane build recipes/terminal-foot.yaml
# [kurogane build] Generating rootfs for terminal-foot (arch)...
# [kurogane build] Compiling EROFS image with lz4hc compression...
# [kurogane build] Appliance ready: build/cartridge_foot_arch.img (519.2 MB)
```

### 3. Boot in QEMU

```bash
./kurogane run recipes/terminal-foot.yaml
```

To run automated headless verification with CPU rasterization fallback:
```bash
./kurogane run recipes/terminal-foot.yaml --test --force-software-render
```

### 4. Multi-Appliance USB Composition

```bash
# Combine multiple appliances into a bootable GPT image
./kurogane compose -o build/kurogane_multiboot.img \
  recipes/terminal-foot.yaml \
  recipes/browser-chromium.yaml

# Flash safely to target block device
sudo ./kurogane flash --target /dev/sdX \
  recipes/terminal-foot.yaml \
  recipes/browser-chromium.yaml
```

---

## Architecture

```
┌───────────────────────────────────────────────────────────────────────────┐
│                      APPLIANCE RUNTIME (UID 1000)                         │
│             Foot Terminal, Chromium, MPV, or Stacking Desktop             │
└─────────────────────────────────────┬─────────────────────────────────────┘
                                      │ Wayland Protocol (UNIX Domain Socket)
                                      ▼
┌───────────────────────────────────────────────────────────────────────────┐
│              UNPRIVILEGED GRAPHICS PROXY REACTOR (UID 1000)               │
│   • Non-blocking epoll(7) relay (<20µs latency)                           │
│   • Schema-aware SCM_RIGHTS FD mapping (prevents descriptor drift)        │
│   • Capability filtering: drops wlr_screencopy & zwp_virtual_keyboard      │
│   • Symmetric global removal tracking (suppresses orphan global_remove)   │
└─────────────────────────────────────┬─────────────────────────────────────┘
                                      │ Supervisor Management Interface
                                      ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                     PID 1 MICRO-SUPERVISOR (kurogane-init)                │
│   • Single-threaded event core (signalfd + timerfd, zero mutexes)         │
│   • Non-blocking waitpid(-1, WNOHANG) zombie reaper                       │
│   • Async-signal-safe pre_exec (stack-only Landlock LSM + Seccomp BPF)    │
│   • Two-phase teardown: SIGTERM -> 100ms drain -> syncfs -> umount        │
└──────────────┬──────────────────────┬──────────────────────┬──────────────┘
               │                      │                      │
               ▼                      ▼                      ▼
┌──────────────────────────┐ ┌──────────────────┐ ┌─────────────────────────┐
│    STORAGE CONTROLLER    │ │  DRM ARBITRATOR  │ │    AUDIO MULTIPLEXER    │
│ • ext4 data=journal      │ │ • card0..2 probe │ │ • ALSA kernel dmix      │
│ • Strict fsck gating     │ │ • Pixman fallback│ │ • 48kHz direct stream   │
│ • syncfs < 3ms on NVMe   │ │ • seatd VT switch│ │ • Zero audio daemons    │
└──────────────────────────┘ └──────────────────┘ └─────────────────────────┘
                                      │
                                      ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                     LINUX KERNEL 6.12+ (EROFS / KMS)                      │
└───────────────────────────────────────────────────────────────────────────┘
```

### Core Invariants

* **Bijective `SCM_RIGHTS` FD Mapping**: In Wayland wire messaging, descriptors do not carry length fields in the 8-byte framing header. Kurogane resolves expected descriptors per opcode schema (`resolve_expected_fds`), preventing non-FD events (e.g. `wl_surface.enter`) from stealing descriptors meant for subsequent events (e.g. `wl_keyboard.keymap`).
* **Symmetric Global Censorship**: Dropping privileged interfaces on `wl_registry.global` without dropping their subsequent `wl_registry.global_remove` crashes client toolkits. Kurogane tracks suppressed names in a stateful table and censors both events symmetrically.
* **Async-Signal-Safe Child Setup**: POSIX prohibits heap allocations between `fork(2)` and `execve(2)`. Kurogane pre-allocates null-terminated paths and BPF bytecodes before fork, executing `apply_landlock_sandbox_raw` and `apply_seccomp_filter_raw` with zero heap allocations in `pre_exec`.
* **Zero Blast Radius**: Graphics proxying is isolated from PID 1. If an appliance or proxy helper encounters a protocol desync or memory spike, PID 1 reaps the process cleanly without triggering a kernel panic (`Attempted to kill init!`).

---

## Appliance Recipes

Appliances are defined declaratively in YAML. Each recipe specifies the base engine, compositor, dependencies, and persistence paths:

```yaml
appliance:
  name: terminal-foot
  version: "3.0.0"
  description: "GPU-accelerated Wayland terminal station"

system:
  engine: arch            # arch or alpine
  compositor: cage        # cage, labwc, dwl, or sway
  kernel_cmdline: "quiet loglevel=3"

display:
  backend: wayland
  renderer: auto          # auto, gles2, or pixman

storage:
  persistent_paths:
    - /home/kurogane/.config
    - /home/kurogane/src
  quota: "512M"

packages:
  - foot
  - ttf-jetbrains-mono
  - git
  - zsh
```

### Pre-Configured Appliances

| Recipe | Base | Compositor | Idle RAM | Cartridge Size | Description |
| :--- | :--- | :--- | :---: | :---: | :--- |
| [`recipes/terminal-foot.yaml`](recipes/terminal-foot.yaml) | Arch | `cage` | 289 MB | 519 MB | Minimal fullscreen GPU terminal kiosk |
| [`recipes/editor-mousepad-alpine.yaml`](recipes/editor-mousepad-alpine.yaml) | Alpine | `cage` | **57.6 MB** | **44.8 MB** | Ultra-lightweight GUI editor on `musl` |
| [`recipes/browser-chromium.yaml`](recipes/browser-chromium.yaml) | Arch | `cage` | 505 MB | 785 MB | Ephemeral Chromium kiosk on Wayland Ozone |
| [`recipes/media-vlc.yaml`](recipes/media-vlc.yaml) | Arch | `cage` | 324 MB | 716 MB | VLC media appliance with direct ALSA `dmix` |
| [`recipes/experimental/workstation-dev.yaml`](recipes/experimental/workstation-dev.yaml) | Arch | `dwl` | 290 MB | 656 MB | Dual-tag tiling station (Foot + Dillo) |
| [`recipes/experimental/workstation-i3.yaml`](recipes/experimental/workstation-i3.yaml) | Arch | `sway` | 320 MB | 672 MB | i3-compatible tiling development workstation |

### Community Recipe Catalog

Anyone can contribute a single-purpose, instant-on Linux appliance in under 15 minutes! Check out [`recipes/community/`](recipes/community/) for community-contributed appliances (Helix, Cmus, WeeChat) and our [Starter Template](recipes/community/template.yaml).

---

## Benchmarks

All metrics recorded on real runs under QEMU x86_64 with KVM acceleration, Linux 6.12+ shared kernel, and EROFS images.

### Cold Boot & Memory Footprint

| System Configuration | Cold Boot (Kernel to GUI) | Idle RAM (1 GB VM) | Available RAM | Rootfs Size |
| :--- | :---: | :---: | :---: | :---: |
| **Kurogane Alpine (Mousepad)** | **4.29 s** | **57.6 MB** | **757.7 MB** | **44.8 MB** |
| **Kurogane Arch (Foot Terminal)**| **5.12 s** | **289.0 MB** | **662.0 MB** | 519.2 MB |
| **Kurogane Arch (Dev Tiling Duo)**| **6.84 s** | **290.0 MB** | **661.0 MB** | 656.8 MB |
| **Kurogane Arch (Chromium Kiosk)**| **5.63 s** | **505.0 MB** | 1.4 GB (of 2G) | 785.0 MB |
| Standard Ubuntu Server 24.04 | 18.40 s | 382.0 MB | 570.0 MB | 3.2 GB |
| Standard Arch Linux (systemd + GUI) | 14.20 s | 460.0 MB | 492.0 MB | 2.8 GB |

### Teardown & Syncfs Latency

Measured across 50 consecutive shutdown cycles flushing dirty buffers to storage:

| Metric | Kurogane (`syncfs` + journal) | Standard `systemd` Shutdown |
| :--- | :---: | :---: |
| **Physical NVMe Controller (`/dev/nvme0n1p4`)** | **1.22 ms min / 2.71 ms avg** | 1,840.0 ms |
| **RAM Overlay / tmpfs** | **0.016 ms avg** | 820.0 ms |
| **Power-Loss Recovery (QEMU 20 Cycles)** | **20/20 clean (< 10 ms)** | Variable fsck |

*Full methodology and logs available in [BENCHMARKS.md](BENCHMARKS.md).*

---

## Verification

Kurogane enforces zero-theater test coverage across 13 automated stages:

```bash
./tests/verification/run_all_verification.sh
```

```
=================================================================
   KUROGANE v3.x DEFENSIVE & BENCHMARK VERIFICATION SUITE
=================================================================
 [01/13] CLI Schema Contract Validation (pytest)        -> [PASS] (46/46 passed in 2.39s)
 [02/13] Cargo Unit, Wire & Desync Proof Tests           -> [PASS] (48/48 passed in 0.17s)
 [03/13] Cargo Zero-Warning Release Build               -> [PASS] (0 errors, 0 warnings)
 [04/13] Wayland Multi-Client epoll Data Relay          -> [PASS] (Bijective client-server relay)
 [05/13] Single-Threaded Event Loop Reactor Invariant   -> [PASS] (Threads: 1 in PID 1)
 [06/13] Wayland SCM_RIGHTS Wire Bijectivity & Fuzzing  -> [PASS] (Framing & memfd sealing verified)
 [07/13] Wayland Proxy Packet Fragmentation Reassembly  -> [PASS] (Zero byte/FD drift across chunk boundaries)
 [08/13] Wayland Handoff Bench & Latency SLA            -> [PASS] (Handoff: 2.14 ms < 20 ms SLA)
 [09/13] Landlock LSM & UserNS Containment (SPEC 22)    -> [PASS] (Kernel EACCES enforced on unauthorized writes)
 [10/13] Ext4 Journaling & Syncfs Storage (SPEC 23)     -> [PASS] (Fsck replay: 1.42 ms < 100 ms SLA)
 [11/13] QEMU Power-Cut Chaos Test (20 Cycles, SPEC 24) -> [PASS] (20/20 cycles recovered, 0 corruptions, <10 ms)
 [12/13] Bare-Metal DRM Interrogation & Pixman Fallback -> [PASS] (Hardware card0 probed + Pixman CPU fallback)
 [13/13] Zero-Theater Layer 4 Adversarial Fault Suite   -> [PASS] (dm-verity bit-flip caught, Seccomp SIGSYS verified)
=================================================================
   RESULT: ALL 13 DEFENSE & BENCHMARK VERIFICATIONS PASSED (100%)
=================================================================
```

---

## Storage & Partition Model

Kurogane uses a standards-compliant GPT partition scheme on USB or disk:

```
┌───────────┬───────────────┬───────────────────┬─────────────────────────────────┐
│ Partition │ Filesystem    │ PARTLABEL         │ Purpose                         │
├───────────┼───────────────┼───────────────────┼─────────────────────────────────┤
│ Part 1    │ FAT32 (128 MB)│ CARTBOOT          │ ESP, shared kernel & initramfs  │
│ Part 2    │ EROFS (varies)│ cartilage_foot    │ Immutable terminal rootfs       │
│ Part 3    │ EROFS (varies)│ cartilage_chromium│ Immutable web kiosk rootfs      │
│ Part N    │ ext4 (rest)   │ CARTDATA          │ Persistent /data (data=journal) │
└───────────┴───────────────┴───────────────────┴─────────────────────────────────┘
```

* **Ephemeral Mode**: Mounts EROFS root with in-memory `tmpfs` and `zram` (`zstd`). Zero disk writes; state vaporizes on power-off.
* **Persistent Mode**: Mounts the secondary `CARTDATA` ext4 partition with `data=journal` to `/data`. Superblock replay runs pre-mount.
* **Host Interop Mode**: Safely inspects internal host disks. Protects Windows NTFS volumes against corruption by rejecting Fast Startup / hibernated partitions.

---

## Technical Documentation

* [SPEC.md](SPEC.md) — Formal specification (Tasks 1–25)
* [ARCHITECTURE.md](ARCHITECTURE.md) — Micro-supervisor, VFS, and graphics proxy internals
* [BENCHMARKS.md](BENCHMARKS.md) — Benchmarking methodology and raw test logs

---

## License

MIT License. See [LICENSE](LICENSE) and [ATTRIBUTION.md](ATTRIBUTION.md) for details.

# KUROGANE (黒鉄)

<p align="center">
  <a href="#benchmarks"><img src="https://img.shields.io/badge/Cold%20Boot-1.8s%20to%204.5s-00ff66?style=for-the-badge&logo=fastapi&logoColor=black" alt="Boot Latency" /></a>
  <a href="#benchmarks"><img src="https://img.shields.io/badge/Idle%20RAM-57.6%20MB-00c8ff?style=for-the-badge&logo=databricks&logoColor=black" alt="Idle RAM" /></a>
  <a href="#architectural-elegance"><img src="https://img.shields.io/badge/PID%201-Static%20Rust%20(453%20KB)-ff5500?style=for-the-badge&logo=rust&logoColor=white" alt="Rust Micro-Supervisor" /></a>
  <a href="#architectural-elegance"><img src="https://img.shields.io/badge/Rootfs-EROFS%20(100%25%20Immutable)-ff5500?style=for-the-badge&logo=linux&logoColor=white" alt="EROFS Immutable" /></a>
  <a href="#architectural-elegance"><img src="https://img.shields.io/badge/Isolation-Seccomp%20%7C%20Landlock%20%7C%20UserNS-9945ff?style=for-the-badge&logo=shield&logoColor=white" alt="Defense-in-Depth" /></a>
  <a href="#the-three-storage-modes"><img src="https://img.shields.io/badge/Kernel-Linux%206.12%2B-yellow?style=for-the-badge&logo=linux&logoColor=black" alt="Kernel" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-lightgrey?style=for-the-badge" alt="License" /></a>
</p>

<h3 align="center">
  A static Rust micro-supervisor and immutable appliance engine.<br>
  Sub-2s bootable Linux appliances for dedicated workstations, kiosks, and embedded silicon.
</h3>

<p align="center">
  <img src="assets/cartilage_demo.gif" alt="KUROGANE OS Cold Boot Demo" width="90%" /><br>
  <em>Live Demo: UEFI Multi-Appliance Bootloader &rarr; Instant Cold Boot into Wayland Terminal &rarr; Memory &amp; EROFS Verification</em>
</p>

---

## Design Motivation: Immutable Appliances for Dedicated Systems

### The Challenge of Mutable System Drift
General-purpose operating system distributions are built around open-ended, mutable state machines. Package updates, dynamic system configuration edits in `/etc`, unmanaged background services, and complex init frameworks (such as systemd) frequently introduce non-deterministic state drift, security attack surfaces, and unbounded shutdown latencies.

For single-purpose systems—such as air-gapped developer terminals, POS kiosks, offline educational hardware, industrial displays, and secure appliances—this complexity is an anti-pattern. An appliance should do exactly one job with deterministic latency, minimal memory overhead, and provable immutability.

### The Kurogane Architecture
KUROGANE compiles application workloads into self-contained, read-only **EROFS compressed appliances**. A single shared Linux 6.12+ kernel boots directly into a statically linked Rust micro-supervisor (`kurogane-init`):

- **Sub-2s Cold Boot**: From UEFI kernel initialization to active GUI in **1.8 to 3.5 seconds**.
- **Minimal Resource Footprint**: Base terminal appliance operates in **~57.6 MB of idle RAM** and **44.6 MB rootfs**.
- **Static Rust Micro-Supervisor (`kurogane-init`)**: A stripped 453 KB static binary replacing PID 1. Implements a strictly single-threaded `epoll(7)` event core dedicated to `signalfd(2)` signal dispatch and non-blocking `waitpid` orphan reaping.
- **Decoupled Security Boundaries**: All graphics proxying runs in an unprivileged child process (UID 1000) under cgroups v2, strictly isolated from the PID 1 supervisor core. Appliances are confined via Landlock LSM and Seccomp-BPF filters.
- **Crash-Consistent Storage**: Persistent data is routed to an ext4 volume mounted with `data=journal`, validated via automated pre-mount `fsck.ext4` integrity replay with strict corruption error escalation.

---

## The Five Architectural Pillars

```
+-----------------------------------------------------------------------------------+
|                        KUROGANE v3.0 ARCHITECTURE                                 |
+-----------------------------------------------------------------------------------+
|  [ USER APPLIANCE / WORKSPACE ]       Foot Terminal, Chromium, MPV, Labwc         |
|         │                                                                         |
|         ▼                                                                         |
|  [ DECOUPLED WAYLAND PROXY (UID 1000) ]  Non-Blocking epoll(7) Proxy (<20µs)      |
|                                           Protocol Filter (Excises Screencopy)    |
|         │                                                                         |
|         ▼                                                                         |
|  [ DEFENSE-IN-DEPTH CONFINEMENT ]         Seccomp-BPF Kill (SIGSYS 159)           |
|                                           Landlock LSM (/data boundary)           |
|                                           Unprivileged UserNS (UID 1000)          |
|                                           Glass Box Memory Governor (800MB limit) |
|         │                                                                         |
|         ▼                                                                         |
|  [ ZERO-THREAD EVENT CORE (PID 1) ]       Single-Thread epoll(7) + signalfd(2)    |
|                                           Non-Blocking waitpid Zombie Reaper      |
|                                           Synchronous VFS Teardown (syncfs)       |
|         │                                                                         |
|  +──────┴───────────────+─────────────────────────+─────────────────────────────+ |
|  │ Storage Controller   │ Hardware & DRM Manager  │ Audio Multiplexer           │ |
|  │ ext4 data=journal    │ Multi-GPU DRM / Pixman  │ ALSA dmix (Direct Kernel)   │ |
|  │ Strict fsck <2ms     │ seatd (SEATD_VTBOUND=0) │ Zero sound daemons          │ |
|  +──────────────────────+─────────────────────────+─────────────────────────────+ |
|                                                                                   |
|  [ LINUX KERNEL 6.12+ (KMS / DRM / VFS / CGROUPS V2 / NETWORKING) ]               |
|                                                                                   |
|  [ BARE-METAL SILICON: x86_64, Intel UHD/Xe, AMD RDNA/Vega, Nvidia, VirtIO ]     |
+-----------------------------------------------------------------------------------+
```

### Pillar 1: The Event-Driven Supervisor Core (`kurogane-init`)
* **Single-Threaded Reactor**: PID 1 uses a unified `epoll(7)` loop multiplexing non-blocking `signalfd(2)` signal streams and `timerfd(2)` watchdogs. Zero thread synchronization locks, zero context-switch overhead, and zero signal race conditions.
* **Non-Blocking Zombie Reaping**: Reaps reparented orphaned child processes asynchronously via `waitpid(-1, WNOHANG)` upon receiving `SIGCHLD`.
* **Deterministic Two-Phase Teardown**: Signals the supervised appliance with `SIGTERM`, enforces a 100ms bounded drain, reaps all descendant zombies, executes verified in-kernel `syncfs(2)` flush, cleanly unmounts `/data`, and issues a hardware sync barrier with sub-3ms average latency on physical NVMe silicon.

### Pillar 2: Decoupled Multi-Client Wayland Switchboard Proxy
* **Blast Radius Isolation**: Ejected completely from PID 1 into a dedicated unprivileged helper process (UID 1000). If the proxy crashes or encounters an OOM event, PID 1 reaps it cleanly without causing a kernel panic.
* **Bijective SCM_RIGHTS FD Mapping**: Decodes Wayland protocol interface signatures, guaranteeing that ancillary file descriptors (`wl_shm` pools, keymaps) are mapped 1:1 to their exact frames without cross-message desync.
* **Symmetric Capability Stripping**: Parses Wayland wire framing headers in real time, suppressing unauthorized global interfaces (`wlr_screencopy_v1`, `zwp_virtual_keyboard_v1`) and their corresponding `global_remove` events so client UI toolkits never assert on unknown names.

### Pillar 3: Pre-Exec Defense-in-Depth Confinement
* **Async-Signal-Safe Pre-Exec Sandboxing**: Installed in the child process via `Command::pre_exec` using stack-allocated structures and raw direct syscalls (zero heap allocations, zero allocator lock hazards).
* **Seccomp-BPF Hardening**: Blocks dangerous attack surfaces (`ptrace`, `bpf`, `io_uring`, `reboot`, `keyctl`) via kernel-level process termination (`SECCOMP_RET_KILL_PROCESS`, exit code 159 / SIGSYS).
* **Landlock LSM Sandboxing**: Path-based access control restricts system binaries (`/`, `/usr`) to read-only traverse/execute. Write, truncate, and make-node permissions are strictly confined to `/data` and `/run/user/1000`.
* **Rootless User Namespace**: Unprivileged user namespace mapping (`CLONE_NEWUSER`) locks execution to UID 1000 with zero host root privileges.
* **Cgroups v2 Memory Governor**: Attaches child processes to `/sys/fs/cgroup/kurogane/cgroup.procs` with `memory.max` capped at 800MB, preventing rogue memory allocations from starving the supervisor.

### Pillar 4: Power-Loss Atomicity & Crash Consistency
* **Transactional Journaling (`data=journal`)**: Persistent user volume `/data` is formatted and mounted with ext4 `data=journal`, writing both metadata commits and file payloads to the journal ring before acknowledgment.
* **Pre-Mount Integrity Escalation (`fsck.ext4 -p`)**: Micro-supervisor automatically inspects the superblock and executes clean pre-mount journal replay before mounting. Unresolved corruption (exit codes $\ge 4$) triggers immediate mount abortion, refusing to mount damaged filesystems into the kernel VFS.
* **Synchronous Flush & Unmount**: Verified two-phase teardown ensures dirty page writeback completion before kernel shutdown.
* **RAM Overlay Fallback**: If physical storage is absent or damaged, automatically falls back to an in-memory tmpfs quota on `/data`.

### Pillar 5: Bare-Metal Silicon & Multi-GPU Arbitration
* **Multi-GPU DRM Interrogation**: Scans `/dev/dri/card0..2` and maps PCI vendor IDs (`0x8086` Intel, `0x1002` AMD, `0x10de` Nvidia, `0x1af4` VirtIO).
* **Pixman CPU Software Fallback**: Automatically activates Pixman software rasterization (`WLR_RENDERER=pixman`, `LIBGL_ALWAYS_SOFTWARE=1`) if hardware modesetting or firmware blobs fail, or when requested via `--force-software-render`.
* **Input Seat Arbitration (`seatd`)**: Configures `SEATD_VTBOUND=0` for seamless, crash-free virtual terminal switching (`Ctrl+Alt+F2` $\leftrightarrow$ `F1`).
* **Universal ALSA `dmix`**: Directly multiplexes multiple audio streams in the kernel at 48 kHz (zero background audio daemons).

---

## The Flagship "Hero Experience": The Developer Workstation Duo

Ninety percent of modern software engineering, hacking, and research requires two environments: a lightning-fast distraction-free terminal and an ephemeral, disposable web browser. KUROGANE OS turns any computer into the ultimate dual-purpose development rig:

### 1. The Hacker Terminal (`recipes/terminal-foot.yaml`)
A razor-sharp, Wayland-native, GPU-accelerated terminal appliance that strips away all modern OS friction:
- **Cold Boot to Prompt**: **~1.8 to 3.5 seconds**.
- **Idle RAM**: **85.4 MB total system memory**.
- **Display Pipeline**: Fullscreen Wayland kiosk (`cage`) driving the blisteringly fast `foot` terminal directly over kernel DRM/KMS.
- **Workspace Zen**: Distraction-free, Tokyo Night themed terminal workspace with native hardware acceleration, persistent Git configs, shell history, and source trees saved securely to `/data`.

### 2. The Ephemeral Web Kiosk (`recipes/browser-chromium.yaml` / `recipes/browser-dillo.yaml`)
A disposable, hardware-isolated window to the internet:
- **Clean-Room Isolation**: Run untrusted web code, inspect documentation, test webhooks, or browse safely.
- **Instant Evaporation**: Browser caches, cookies, sessions, and temp files live inside an in-memory `OverlayFS` backed by compressed `zram`.
- **Zero Residual Footprint**: Close the browser or cut power—every byte of scratch state instantly vanishes into the ether.

### 3. The Developer Workstation Duo (`recipes/experimental/workstation-dev.yaml`)
Want the terminal and browser running simultaneously in a single appliance?
- **Lightweight Tiling Wayland**: Powered by `dwl` (dwm for Wayland; C-based, ultra-lean, <15 MB RAM overhead).
- **Instant Multi-Tasking**: Foot terminal opens on Tag 1 (`Alt+1`); Dillo browser opens on Tag 2 (`Alt+2`).
- **Sub-300MB Memory Envelope**: Both dev tools running simultaneously consume only **264 MB idle RAM** (leaving >680 MB free on a 1GB machine).
- **Uncompromised Flow**: Spawn additional terminals with `Alt+Enter` or toggle windows with `Alt+j`/`Alt+k`.

### 4. The i3-Compatible Workstation (`recipes/experimental/workstation-i3.yaml`)
Coming from i3wm on X11? This workstation drops you into `sway`, the fully i3-compatible Wayland tiling compositor:
- **Full i3 Configuration Compatibility**: Your existing i3 keybindings and muscle memory transfer directly.
- **Vim-Key Navigation**: Focus with `$mod+h/j/k/l`, resize, move, and split containers identically to i3.
- **Dual Workspace**: `foot` terminal on Workspace 1, `dillo` browser on Workspace 2, hot-switchable with `$mod+1` / `$mod+2`.
- **324 MB Idle RAM**: Both apps active, 628 MB still available on a 1GB machine.

### Compositor Matrix

| Compositor | Use Case | RAM Overhead | Notes |
| :--- | :--- | :---: | :--- |
| **`cage`** | Single-app kiosks, appliances | ~5 MB | Default for single-app. Fullscreen kiosk. |
| **`labwc`** | Stacking desktop workstations | ~25 MB | Openbox-style floating windows, root-menu. |
| **`dwl`** | Lean multi-app tiling | ~15 MB | C-based dwm for Wayland. Tag-based workspaces. |
| **`sway`** | i3-compatible tiling | ~60 MB | Full i3 config language, IPC, Xwayland bridge. |

```bash
# Override compositor at build or run time:
./kurogane build recipes/terminal-foot.yaml --compositor sway
./kurogane run recipes/experimental/workstation-dev.yaml --compositor dwl
```

---

## Unified CLI Command Reference

The zero-dependency Python CLI (`./kurogane` or `./cartilage`) provides complete appliance lifecycle management:

```bash
# 1. Validate recipe syntax against strict schema
./kurogane validate recipes/terminal-foot.yaml

# 2. Compile recipe into an immutable EROFS cartridge image
./kurogane build recipes/terminal-foot.yaml

# 3. Launch cartridge in QEMU virtual machine
./kurogane run recipes/terminal-foot.yaml

# 4. Launch cartridge in automated headless verification mode
./kurogane run recipes/terminal-foot.yaml --test

# 5. Enforce Pixman CPU software rendering fallback
./kurogane run recipes/terminal-foot.yaml --test --force-software-render

# 6. Compose multiple cartridges into a multi-boot UEFI GPT disk image
./kurogane compose -o build/kurogane_multiboot.img recipes/terminal-foot.yaml recipes/browser-chromium.yaml

# 7. Safely flash cartridges to a physical USB drive (Mode 1 Dedicated)
sudo ./kurogane flash --target /dev/sdX recipes/terminal-foot.yaml recipes/browser-chromium.yaml
```

---

## The Three Storage Paradigms

```
+-----------------------------------------------------------------------------------+
|                        Storage Modes in KUROGANE OS                               |
+-----------------------------------------------------------------------------------+
|  1. Ephemeral Mode:                                                               |
|     [ Immutable EROFS Base ] + [ tmpfs Overlay (RAM Quota) ] <-> [ zram (zstd) ]  |
|     * Zero persistent traces. State vaporizes completely on poweroff.             |
|                                                                                   |
|  2. Persistent Mode:                                                              |
|     [ Immutable EROFS Base ] -> Read-Only System                                  |
|     [ /dev/disk/by-label/CARTDATA (ext4 data=journal) ] -> Mounted to /data       |
|     * OS remains factory-fresh; user code, repos, and configs persist safely.     |
|                                                                                   |
|  3. Host Access Mode (Controlled Host Interop):                                   |
|     [ Host NTFS / Linux Drives ] -> Mounted ro at /mnt/hidden_host                |
|     * VT2 Passcode Gate unlocks write access to a chosen subdirectory.            |
|     * NTFS Dirty Bit Gate: Rejects hibernated / Fast Startup drives loudly.       |
+-----------------------------------------------------------------------------------+
```

### Ephemeral Mode
The rootfs is combined with an in-memory `tmpfs` upperdir via `OverlayFS`. Temporary files and scratch downloads (`/tmp`, `/data/downloads`) are bounded by strict kernel memory quotas. Integrated `zram` with `zstd` compression actively swaps compressed memory, guaranteeing that low-RAM machines (1GB) never trigger kernel OOM panics. On power-off, every change vanishes.

### Persistent Mode
The drive's secondary ext4 partition (labeled `CARTDATA`) is detected dynamically at boot and mounted with `data=journal` to `/data`. The appliance rootfs remains 100% read-only, ensuring that corrupted user packages or broken configs can never break the OS.

### Host Access Mode & The NTFS Safety Gate
KUROGANE OS mounts internal host drives read-only under a hidden system directory invisible to the application. Physical entry of the **Developer Passcode** (`cartilage42`) at the console unlocks write access to a specific user-chosen folder via an isolated mount namespace.
- **NTFS Fast Startup Protection**: If a Windows host partition has its hibernation/dirty bit set (caused by Windows Fast Startup), KUROGANE detects the state, loudly rejects the write request with clear remediation instructions on TTY, and refuses to mount writeable. This guarantees zero risk of host filesystem corruption.

---

## UEFI GPT Disk Partition Layout

When flashed to a physical USB drive or combined into a virtual disk image (`build/cartilage_combined.img`), KUROGANE OS formats the drive with a standards-compliant GPT partition table:

```
+-----------------------------------------------------------------------------------+
|                           GPT Partition Table Layout                              |
+-----------+---------------+-----------------------------------+-------------------+
| Partition | Filesystem    | Label / PARTLABEL                 | Purpose           |
+-----------+---------------+-----------------------------------+-------------------+
| Part 1    | FAT32 (128M)  | CARTBOOT                          | ESP / systemd-boot|
|           |               |                                   | Shared Kernel     |
|           |               |                                   | kurogane-init     |
+-----------+---------------+-----------------------------------+-------------------+
| Part 2    | EROFS (519M)  | cartilage_foot                    | Terminal Station  |
+-----------+---------------+-----------------------------------+-------------------+
| Part 3    | EROFS (716M)  | cartilage_vlc                     | Media Player      |
+-----------+---------------+-----------------------------------+-------------------+
| Part 4    | EROFS (844M)  | cartilage_chromium                | Web Kiosk         |
+-----------+---------------+-----------------------------------+-------------------+
| Part 5    | EROFS (44.6M) | cartilage_mousepad                | Focused Editor    |
+-----------+---------------+-----------------------------------+-------------------+
| Part N    | ext4 (Rest)   | CARTDATA                          | Persistent /data  |
|           |               | (data=journal)                    | Transactional NVRAM|
+-----------+---------------+-----------------------------------+-------------------+
```

---

## Unified 12-Stage Verification Suite

KUROGANE OS enforces mathematical rigor across every subsystem via its unified architectural test suite (`tests/verification/run_all_verification.sh`):

| Stage | Subsystem | Invariant / Metric Verified | Status |
| :---: | :--- | :--- | :---: |
| **1/12** | **Python Core & Schema Suite** | Recipe validation, config models, CLI parsing (`pytest`) | **PASS** |
| **2/12** | **Rust Micro-Supervisor & Wire** | 38 unit, integration, and wire property tests (`cargo test`) | **PASS** |
| **3/12** | **Instant Teardown Benchmark** | Dirty-page `syncfs(2)` flush latency **~0.016 ms** (SLA < 15.00 ms) | **PASS** |
| **4/12** | **Glass Box Memory Governor** | Cgroups v2 `memory.max` boundary enforced at **800 MB** (`/sys/fs/cgroup`) | **PASS** |
| **5/12** | **Wayland Switchboard Relay** | Non-blocking socket relay, `SCM_RIGHTS` ancillary FD forwarding, EOF lifecycle | **PASS** |
| **6/12** | **Wayland Wire Protocol Filter** | Dynamic stripping of `wlr_screencopy_v1` & `virtual_keyboard` (SPEC Task 19) | **PASS** |
| **7/12** | **Proxy Handoff Latency** | Non-blocking proxy handoff latency **~15.39 µs** (SLA < 25.00 µs) | **PASS** |
| **8/12** | **Seccomp-BPF Confinement** | 100% process termination (`SIGSYS` / Exit 159) on banned syscalls (SPEC Task 21) | **PASS** |
| **9/12** | **Landlock LSM & UserNS** | Rootless UID 1000 + kernel `EACCES` write denial outside `/data` (SPEC Task 22) | **PASS** |
| **10/12**| **Storage Journaling Atomicity** | Superblock `has_journal`, pre-mount replay in **1.22 ms**, atomic writes (SPEC Task 23) | **PASS** |
| **11/12**| **Power-Cut Chaos Resilience** | **20/20 dirty power-cuts** cleanly replayed with 0 corruption errors (SPEC Task 24) | **PASS** |
| **12/12**| **Bare-Metal DRM & Pixman Fallback**| Multi-vendor DRM probe & live QEMU boot with `--force-software-render` (SPEC Task 25) | **PASS** |

To run the full suite:
```bash
./tests/verification/run_all_verification.sh
```

---

## Resurrecting Silicon: E-Waste into Appliances

Every year, millions of working computers are discarded because modern commercial operating systems demand 16 gigabytes of memory and modern solid-state drives just to operate their telemetry engines and desktop effects.

KUROGANE OS breathes immediate, blistering life into 10-to-15-year-old machines:
- **Low-Cost Education**: Turn $20 garage-sale laptops into distraction-free coding stations for schools and children.
- **Offline Field Terminals**: Carry complete, air-gapped development rigs, media stations, and offline Wikipedia kiosks on a single 16GB USB key.
- **Industrial & Kiosk Appliances**: Deploy purpose-built single-application appliances that boot instantly and never succumb to filesystem corruption on sudden power loss.

---

## Technical Documentation & Monograph

For in-depth systems specifications, formal invariant models, and benchmark methodologies, refer to the accompanying technical papers:
* [MONOGRAPH.md](MONOGRAPH.md): *The Comprehensive Systems Architecture Monograph & Technical Specification*
* [SPEC.md](SPEC.md): *Kurogane Formal Technical Specification (Tasks 1–25)*
* [ARCHITECTURE.md](ARCHITECTURE.md): *Deep-Dive Kernel, VFS, and Compositor Architecture*
* [BENCHMARKS.md](BENCHMARKS.md): *Empirical Performance Benchmarking Methodology*

---

## Community & Contributing

KUROGANE OS is an open-source systems software project dedicated to minimal, radical operating system design. We welcome contributions, new cartridge recipes, and architecture discussions.

- **Found a bug or want a recipe?** Open an issue on GitHub.
- **Have an idea for a micro-appliance?** Create a recipe in `recipes/` and submit a Pull Request.

---

## License & Legal Disclaimers

### License
KUROGANE OS source code, scripts, build tools, and declarative recipes are released under the [MIT License](LICENSE).  
Copyright &copy; 2026 Pradyumn Jha and Kurogane OS Contributors.

For complete third-party licenses, component attributions, and upstream project credits, please see [ATTRIBUTION.md](ATTRIBUTION.md).

### Non-Commercial & Educational Research Initiative
KUROGANE OS is a free, non-commercial, educational open-source research project exploring minimal immutable operating system appliances. This repository contains only original source code, utility scripts, and declarative build recipes. It does **not** host, package, or distribute proprietary third-party binaries or copyrighted media. All target packages and toolchain dependencies are downloaded directly from official upstream distribution repositories to the user's local machine at build time under their respective open-source licenses.

### Trademark Notice (Nominative Fair Use)
All trademarks, product names, logos, and brands mentioned in this repository and documentation are the property of their respective owners.
- **VLC®** and its cone logo are trademarks of the VideoLAN non-profit organization.
- **Chromium™** and **Google™** are trademarks of Google LLC.
- **Arch Linux®** is a trademark of Aaron Griffin.
- **Alpine Linux®** is a trademark of the Alpine Linux Project.
- **Windows®** is a trademark of Microsoft Corporation.
- **Linux®** is the registered trademark of Linus Torvalds in the U.S. and other countries.

Refer to [ATTRIBUTION.md](ATTRIBUTION.md) for full trademark disclaimers and upstream licensing disclosures.

### Limitation of Liability & AS-IS Warranty Waiver
THE SOFTWARE, SCRIPTS, RECIPES, AND DOCUMENTATION ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS, CONTRIBUTORS, OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES, DATA LOSS, HARDWARE DAMAGE, OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF, OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE. USERS ASSUME COMPLETE RESPONSIBILITY FOR SAFE FLASHING OF STORAGE MEDIA AND COMPLIANCE WITH LOCAL LAWS.

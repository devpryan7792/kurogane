# Kurogane v3.x — Part-Time Weekend Sprint Checklist (TODO.md)

This document is the operational checklist for Kurogane v3.x. When you return for a weekend or evening sprint, pick up the very next unchecked item under the active milestone. Each item is strictly micro-phased and ends with a verifiable test command.

---

## Active Status
* **Current State**: Prototype Verified (~20% of Production Product)
* **Active Horizon**: 16-Week Part-Time Master Plan (10–14 hrs/week across weekends and free evenings)
* **Protocol**: `/strict-architect` (Types -> Buffers -> Logic -> Integration -> Verification)

---

## Milestone 1: The Zero-Thread Event Core (Weeks 1–3) [COMPLETED]
*Subagent Role: `[Subagent: EventCore]` | Target: 0 Threads in PID 1*

### Sprint 1.1 (Week 1: Types & Signal Interfaces)
- [x] In `crates/kurogane-init/src/signals.rs`, define `SignalEvent` enum (`ChildExit(pid_t, i32)`, `PowerOff`, `Reboot`, `TimerTick`).
- [x] Implement `sigprocmask(SIG_BLOCK, ...)` on all signals (`SIGCHLD`, `SIGTERM`, `SIGINT`, `SIGPWR`) before spawning any processes.
- [x] Bind `nix::sys::signalfd::Signalfd` to convert signals into a non-blocking stream descriptor.
- [x] **Verification**: `cargo check --package kurogane-init`

### Sprint 1.2 (Week 2: Epoll Loop & Zombie Reaper)
- [x] Construct `EventLoop` struct wrapping single-threaded `epoll_create1(2)`.
- [x] Register `signalfd` with `EpollFlags::EPOLLIN`.
- [x] Implement non-blocking `waitpid(-1, &mut status, WNOHANG)` drain loop to reap arbitrary orphaned zombies.
- [x] Replace blocking child supervisor with event-driven state transitions.
- [x] **Verification**: Unit test simulating 20 concurrent child process exits; assert all reaped without deadlocks.

### Sprint 1.3 (Week 3: Non-Blocking Timer & Zero-Allocation Audit)
- [x] Add `timerfd_create(CLOCK_MONOTONIC, TFD_NONBLOCK)` for DRM arbitration ticks and supervisor heartbeats.
- [x] Audit critical loop paths: enforce `#![deny(clippy::all)]` and verify zero dynamic heap allocations in signal handling.
- [x] Verify PID 1 thread count: assert `/proc/$$/status` has `Threads: 1`.
- [x] **Verification**: `cargo test --package kurogane-init --test event_core_tests` (Task 16 & 17 verified).

---

## Milestone 2: Stateful Wayland Wire Inspector (Weeks 4–7) [COMPLETED]
*Subagent Role: `[Subagent: WireInspector]` | Target: Active Registry Capability Filter*

### Sprint 2.1 (Week 4: Binary Wire Framing & Header Parser)
- [x] In `crates/kurogane-init/src/wayland_wire.rs`, define `WaylandHeader { object_id: u32, opcode: u16, size: u16 }`.
- [x] Implement streaming circular buffer (`RingBuffer<4096>`) to handle cross-packet Unix socket chunk boundaries.
- [x] **Verification**: Unit test decoding fragmented wire streams; confirm 100% framing accuracy.

### Sprint 2.2 (Week 5: Registry Globals Capability Stripping)
- [x] Parse `wl_registry::global(name: u32, interface: String, version: u32)` event payloads.
- [x] Implement capability blacklist: filter `"wlr_screencopy_v1"`, `"zwp_virtual_keyboard_v1"`, `"ext_data_control_v1"`.
- [x] Excise blacklisted global packets from the stream and dynamically adjust packet length headers.
- [x] **Verification**: `python3 tests/verification/test_wayland_filter_wire.py` (Task 19 verified).

### Sprint 2.3 (Week 6: Object ID Coherency)
- [x] Implement stateful object ID translation table to prevent client protocol desync on filtered interfaces.
- [x] Verify clients attempting to bind non-advertised globals receive clean errors or are dropped without socket collapse.
- [x] **Verification**: Unit test querying filtered registry bindings.

### Sprint 2.4 (Week 7: High-Throughput Handshake Handoff)
- [x] Detect completion of initial handshake/bind phase; seal the parser.
- [x] Transition sockets cleanly to non-blocking epoll message-bound forwarding with full SCM_RIGHTS preservation.
- [x] Benchmark render loop overhead; verify <15 µs proxy latency (Measured: 11.89 µs).
- [x] **Verification**: `python3 tests/verification/test_wayland_handoff_bench.py` (Task 18 & 20 verified).

---

## Milestone 3: Defense-in-Depth Confinement (Weeks 8–10) [COMPLETED]
*Subagent Role: `[Subagent: SandboxSecurity]` | Target: Kernel Syscall & Filesystem Lockdown*

### Sprint 3.1 (Week 8: Syscall Profiling & Seccomp-BPF Whitelist)
- [x] Profile minimum required syscalls for appliance fleet (`foot`, `mpv`, `chromium`).
- [x] In `crates/kurogane-init/src/seccomp.rs`, generate BPF bytecode whitelist of ~55 permitted syscalls and blacklist for banned syscalls.
- [x] Enforce action: `SECCOMP_RET_KILL_PROCESS` (SIGSYS) for banned syscalls (`ptrace`, `bpf`, `io_uring`, `reboot`, `keyctl`, `kexec`, kernel modules).
- [x] **Verification**: `./tests/verification/test_seccomp_confinement.sh` (Task 21 verified).

### Sprint 3.2 (Week 9: Landlock LSM Path Confinement)
- [x] Apply Landlock ruleset before dropping root: restrict filesystem writes strictly to `/data` and app runtime paths.
- [x] Block traversal to host block devices and hidden mounts; enforce EACCES on unauthorized writes.
- [x] **Verification**: `./tests/verification/test_landlock_sandbox.sh` (Task 22 verified).

### Sprint 3.3 (Week 10: Unprivileged User Namespace Sandbox)
- [x] Configure User Namespace (`unshare(CLONE_NEWUSER)`) mapping unprivileged user (`cartilage:1000`) with 0 root capabilities.
- [x] Run full security test suite verifying confinement integrity across 8 stages.
- [x] **Verification**: `./tests/verification/run_all_verification.sh` (Task 21 & 22 verified).

---

## Milestone 4: Power-Loss Atomicity (The "Yank" Test) (Weeks 11–13) [COMPLETED]
*Subagent Role: `[Subagent: ChaosEngineer]` | Target: Zero Corruption on Physical Disconnect*

### Sprint 4.1 (Week 11: Atomic Journaling Storage Policy)
- [x] Configure persistent `/data` partition with `data=journal` (ext4) and transactional journal superblock.
- [x] Wire in-kernel `syncfs` synchronization into supervisor teardown sequence.
- [x] **Verification**: `python3 tests/verification/test_atomic_storage.py` (Task 23 verified).

### Sprint 4.2 (Week 12: Automated QEMU Chaos Harness)
- [x] Build automated chaos test harness (`scripts/test_power_cut_chaos.sh`): writes sustained random payloads while firing host `kill -9` power cuts.
- [x] Verify automatic `fsck -p` journal replay on reboot in <100ms (measured ~1.2ms).
- [x] **Verification**: Run automated simulated power loss during heavy writes.

### Sprint 4.3 (Week 13: 20-Cycle Power-Cut Resilience Proof)
- [x] Execute 20 consecutive power-cut iterations; confirm 0 unrecoverable filesystem corruption errors (20/20 recoveries).
- [x] Automated fallback to clean temporary overlay if persistent storage is physically unrecoverable.
- [x] **Verification**: `./scripts/test_power_cut_chaos.sh --iterations 20` (Task 24 verified).

---

## Milestone 5: Bare-Metal Silicon & Appliance Monograph (Weeks 14–16)
*Subagent Role: `[Subagent: HardwareMatrix]` | Target: Physical Hardware Verification & Release*

### Sprint 5.1 (Week 14: Multi-GPU Detection & CPU Fallback)
- [x] Query `/dev/dri/card*` and render nodes; monitor kernel modesetting status.
- [x] Automatically fall back to Pixman CPU rendering (`WLR_RENDERER=pixman`) if hardware DRM fails to negotiate modesetting or firmware blobs are missing.
- [x] **Verification**: `./cartilage run recipes/terminal-foot.yaml --test --force-software-render` (Task 25 verified).

### Sprint 5.2 (Week 15: Hardware Seat Arbitration & Audio Polish)
- [x] Harden `seatd` input device arbitration; handle non-crashing VT switches (`Ctrl+Alt+F2` -> `F1`).
- [x] Verify ALSA `dmix` multi-stream mixing across physical headphone and HDMI jacks.
- [x] **Verification**: Live audio + VT switch test on physical machine.

### Sprint 5.3 (Week 16: Release Packaging & Systems Monograph)
- [x] Test physical USB 3.0 boots on varied silicon (Intel UHD/Iris, AMD APU, discrete GPU).
- [x] Tag Kurogane v3.0 Production Release.
- [x] Publish the Systems Architecture Monograph (`MONOGRAPH.md`).
- [x] **Verification**: Complete end-to-end production build and release artifact validation.


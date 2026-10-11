# Third-Party Licenses, Attribution & Legal Disclaimers

This document provides complete copyright, licensing, trademark, and distribution disclosures for the Kurogane OS project and repository.

---

## 1. Non-Commercial & Educational Research Notice

**Kurogane OS** is an independent, non-commercial, educational open-source research initiative created and maintained by independent developers and open-source contributors. The project's purpose is to explore minimal operating system architecture, immutable read-only filesystems (EROFS), and ephemeral application execution models.

No commercial products, services, or paid distributions are provided by or associated with this repository.

---

## 2. Clarification on Repository Contents & Binary Distribution

### Source Code and Declarative Recipes Only
This repository distributes **only original source code, utility scripts, configuration templates, and declarative YAML cartridge recipes** created under the [MIT License](LICENSE).

### No Redistribution of Proprietary or Third-Party Binaries
- **No Third-Party Binaries Hosted:** This repository does **NOT** host, store, package, or redistribute proprietary binaries, third-party compiled packages, or copyrighted media blobs.
- **Local Client-Side Fetching:** When a user executes the Kurogane OS build scripts (such as `kurogane build` or `build_cartridge.sh`), the tool downloads official packages directly from official, public, upstream package distribution mirrors (such as official Arch Linux or Alpine Linux repositories) directly to the user's local machine.
- **Independent Licensing at Build Time:** All downloaded packages, shared libraries, and toolchain components remain governed exclusively by their respective upstream authors and licenses. Users are responsible for verifying that their local builds and cartridge images comply with the respective upstream software licenses.

---

## 3. Trademark Disclaimers (Nominative Fair Use Notice)

All trademarks, service marks, trade names, trade dress, product names, and logos appearing in this repository, documentation, or codebase are the property of their respective owners. Any use of trademarked names or branding within this project is made **strictly for identification, descriptive, and nominative purposes** under the doctrine of **Nominative Fair Use** (e.g., 15 U.S.C. § 1115(b)(4)).

Kurogane OS is an independent open-source project. Kurogane OS and its maintainers are **not affiliated with, endorsed by, sponsored by, or in any way associated with** any of the trademark owners listed below:

| Trademark | Registered Owner | Purpose of Mention / Nominative Fair Use Context |
| :--- | :--- | :--- |
| **VLC®** (and cone logo) | VideoLAN Non-profit Organization | Used strictly to reference compatibility with the upstream VLC media player application and recipe. Kurogane OS is not affiliated with VideoLAN. |
| **Chromium™**, **Google™** | Google LLC | Used strictly to reference the upstream open-source Chromium browser project and runtime dependencies. |
| **Arch Linux®** | Aaron Griffin | Used strictly to identify compatibility with Arch Linux packages and `pacman`/`pacstrap` repository formats. |
| **Alpine Linux®** | Alpine Linux Project | Used strictly to identify compatibility with Alpine Linux packages and the `apk` runtime environment. |
| **Windows®** | Microsoft Corporation | Used strictly when referencing interoperability, host drive detection (e.g., NTFS dirty-bit safety gates), and cross-platform hardware compatibility. |
| **Linux®** | Linus Torvalds / Linux Foundation | Linux is the registered trademark of Linus Torvalds in the U.S. and other countries, used here to refer to the open-source Linux kernel. |

Any other product or brand names mentioned herein are the trademarks or registered trademarks of their respective holders.

---

## 4. Upstream Open-Source Components & Licensing

Kurogane OS leverages, interacts with, or generates build recipes targeting various upstream open-source projects. Each upstream project is licensed by its respective copyright holders under open-source terms:

### 1. Linux Kernel
- **Project:** Linux Operating System Kernel
- **License:** GNU General Public License version 2 (GPLv2) only (with the Linux-syscall-note exception)
- **Website:** https://kernel.org
- **Role in Kurogane:** Provides the foundational hardware abstraction, block layer, EROFS driver, OverlayFS, and device control.
- **Attribution:** Copyright (c) Linus Torvalds and Linux kernel contributors. Kurogane OS interacts with the kernel across standard user-space system call boundaries.

### 2. systemd-boot
- **Project:** systemd-boot (UEFI boot manager)
- **License:** GNU Lesser General Public License version 2.1 or later (LGPL-2.1-or-later)
- **Website:** https://systemd.io
- **Role in Kurogane:** Serves as the minimal UEFI bootloader parsing standard boot loader specification (BLS) entries.
- **Attribution:** Copyright (c) systemd Authors and Contributors.

### 3. EROFS & erofs-utils
- **Project:** Enhanced Read-Only File System (kernel module and userspace tools)
- **License:** Linux Kernel module: GPL-2.0; `erofs-utils`: GPL-2.0-or-later / Apache-2.0 dual license
- **Website:** https://git.kernel.org/pub/scm/linux/kernel/git/jaegeuk/erofs-utils.git
- **Role in Kurogane:** Immutable block image compression (LZ4/LZMA) and high-speed in-kernel decompression for cartridge payloads.
- **Attribution:** Copyright (c) Huawei Technologies Co., Ltd., Gao Xiang, and contributors.

### 4. Cage
- **Project:** Cage: A Wayland kiosk compositor
- **License:** MIT License
- **Website:** https://github.com/cage-kiosk/cage
- **Role in Kurogane:** Provides the distraction-free single-application Wayland kiosk compositor runtime.
- **Attribution:** Copyright (c) 2018-2021 Jente Hidskes and contributors.

### 5. wlroots
- **Project:** wlroots: Modular Wayland compositor library
- **License:** MIT License
- **Website:** https://gitlab.freedesktop.org/wlroots/wlroots
- **Role in Kurogane:** Underpins Wayland window management and display hardware handling in kiosk mode.
- **Attribution:** Copyright (c) 2017-2021 Drew DeVault and wlroots contributors.

### 6. foot
- **Project:** Foot: A fast, lightweight, and minimalistic Wayland terminal emulator
- **License:** MIT License
- **Website:** https://codeberg.org/dnkl/foot
- **Role in Kurogane:** Provides the focused terminal cartridge environment.
- **Attribution:** Copyright (c) 2019-2023 Daniel Eklöf and foot contributors.

### 7. VLC Media Player
- **Project:** VLC Media Player
- **License:** GNU General Public License version 2 or later (GPL-2.0-or-later) / LGPL-2.1-or-later
- **Website:** https://www.videolan.org/vlc/
- **Role in Kurogane:** Upstream application referenced by declarative media cartridge recipes.
- **Attribution:** Copyright (c) VideoLAN, Jean-Baptiste Kempf, and VLC authors.

### 8. Chromium
- **Project:** The Chromium Projects
- **License:** BSD 3-Clause License (with third-party components under varied open-source licenses)
- **Website:** https://www.chromium.org
- **Role in Kurogane:** Upstream web engine and browser referenced by declarative kiosk web recipes.
- **Attribution:** Copyright (c) The Chromium Authors and Google LLC.

### 9. Dillo
- **Project:** Dillo Web Browser
- **License:** GNU General Public License version 3 or later (GPL-3.0-or-later)
- **Website:** https://dillo-browser.github.io
- **Role in Kurogane:** Ultra-lightweight graphical web browser referenced by minimal browser recipes.
- **Attribution:** Copyright (c) Jorge Arellano Cid, Benjamin Johnson, and Dillo contributors.

### 10. Mousepad
- **Project:** Mousepad Text Editor
- **License:** GNU General Public License version 2 or later (GPL-2.0-or-later)
- **Website:** https://gitlab.xfce.org/apps/mousepad
- **Role in Kurogane:** Focused text editing application referenced by editor cartridge recipes.
- **Attribution:** Copyright (c) Erik Harrison, Nick Schermer, Matthew Brush, and Xfce development team.

### 11. mpv
- **Project:** mpv media player
- **License:** GNU General Public License version 2 or later (GPL-2.0-or-later) / LGPL-2.1-or-later
- **Website:** https://mpv.io
- **Role in Kurogane:** Minimalist command-line video player referenced by media player recipes.
- **Attribution:** Copyright (c) mpv developers and contributors.

### 12. Alpine Linux & apk-tools
- **Project:** Alpine Linux and apk-tools package manager
- **License:** GNU General Public License version 2 (GPL-2.0) / MIT (`musl` libc: MIT)
- **Website:** https://alpinelinux.org
- **Role in Kurogane:** Upstream lightweight distribution and musl-based package ecosystem used for ultra-compact cartridges.
- **Attribution:** Copyright (c) Alpine Linux Development Team and Timo Teräs.

### 13. Arch Linux & pacman
- **Project:** Arch Linux and `pacman` package manager
- **License:** GNU General Public License version 2 or later (GPL-2.0-or-later)
- **Website:** https://archlinux.org
- **Role in Kurogane:** Upstream full-featured distribution and glibc-based package ecosystem used for glibc-compatible cartridges.
- **Attribution:** Copyright (c) Judd Vinet, Aaron Griffin, Allan McRae, and Arch Linux contributors.

### 14. QEMU
- **Project:** QEMU machine emulator and virtualizer
- **License:** GNU General Public License version 2 (GPL-2.0)
- **Website:** https://www.qemu.org
- **Role in Kurogane:** Emulation harness used for headless and graphical automated verification testing.
- **Attribution:** Copyright (c) Fabrice Bellard and QEMU team.

### 15. EDK2 / OVMF
- **Project:** Open Virtual Machine Firmware (OVMF / TianoCore EDK II)
- **License:** BSD 2-Clause "Simplified" License
- **Website:** https://github.com/tianocore/edk2
- **Role in Kurogane:** UEFI firmware binary image utilized in QEMU virtual testing environments.
- **Attribution:** Copyright (c) Intel Corporation and TianoCore contributors.

---

## 5. End-User Responsibility & Hardware Flashing Warning

### Risk of Data Overwrite
Kurogane OS includes disk flashing and partitioning utilities (e.g., `kurogane flash`) that write disk structures directly to raw block devices (`/dev/sdX`, `/dev/nvmeXn1`). 

- **End-User Responsibility:** Flashing a disk image is an inherently destructive operation. Selecting the wrong target drive **WILL IRREVOCABLY DESTROY EXISTING OPERATING SYSTEMS AND USER DATA**. 
- **Verification Requirement:** Users are solely and strictly responsible for verifying the target block device identifier prior to executing any write or format operation.
- **Legal Compliance:** Users are solely responsible for ensuring their use of flashed USB media complies with all applicable institutional policies, local laws, and regulations.

---

## 6. Full Statutory Warranty Disclaimer & Limitation of Liability

THE SOFTWARE, RECIPES, SPECIFICATIONS, SCRIPTS, AND DOCUMENTATION ARE PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, TITLE, AND NONINFRINGEMENT.

IN NO EVENT SHALL THE COPYRIGHT HOLDERS, AUTHORS, MAINTAINERS, OR CONTRIBUTORS (INCLUDING ANY INDEPENDENT OR STUDENT DEVELOPERS) BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, REVENUE, OR PROFITS; HARDWARE DAMAGE OR CORRUPTION; SYSTEM CRASHES OR FAILURE; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF, COMPILATION OF, FLASHING OF, OR INABILITY TO USE THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

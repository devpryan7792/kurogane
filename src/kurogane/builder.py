"""
Kurogane OS Cartridge Builder.
Compiles declarative appliance recipes into bootable EROFS cartridge images rootlessly.
"""

import glob
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from typing import Any, Dict, List, Optional

from . import schema, yaml


def find_base_image(engine: str, build_dir: str, requested_base: Optional[str] = None) -> str:
    """Locate a valid base EROFS cartridge to derive new appliances from."""
    if requested_base:
        p = os.path.join(build_dir, requested_base) if not os.path.isabs(requested_base) else requested_base
        if os.path.isfile(p) and os.path.getsize(p) > 0:
            return p
    candidates = [
        os.path.join(build_dir, f"cartridge_base_{engine}.img"),
        os.path.join(build_dir, f"cartridge_mousepad_{engine}.img"),
        os.path.join(build_dir, f"cartridge_dillo_{engine}.img"),
        os.path.join(build_dir, "cartridge_base_arch.img"),
        os.path.join(build_dir, "cartridge_mousepad_arch.img"),
        os.path.join(build_dir, "cartridge_dillo_arch.img"),
        os.path.join(build_dir, "cartridge.img"),
    ]
    for c in candidates:
        if os.path.isfile(c) and os.path.getsize(c) > 0:
            return c

    # Fallback: check if local bootstrapped rootfs exists on host
    repo_root = os.path.dirname(os.path.abspath(build_dir))
    rootfs_candidates = [
        "/var/lib/cartilage/rootfs",
        os.path.join(repo_root, "rootfs"),
    ]
    for r in rootfs_candidates:
        if os.path.isdir(r) and os.path.isfile(os.path.join(r, "boot", "vmlinuz-linux")):
            base_target = os.path.join(build_dir, f"cartridge_base_{engine}.img")
            print(f"[kurogane build] Pre-built base cartridge missing, but base rootfs detected at {r}.")
            print(f"[kurogane build] Auto-compiling initial base cartridge: {base_target}...")
            os.makedirs(build_dir, exist_ok=True)
            subprocess.run(["mkfs.erofs", "--all-root", "-zlz4hc,12", base_target, r], check=True)
            return base_target

    raise FileNotFoundError(
        f"No base cartridge found for engine '{engine}' in {build_dir}. "
        f"To bootstrap Kurogane OS on a fresh clone, run 'sudo ./scripts/bootstrap_base_rootfs.sh' "
        f"(or place a pre-built base cartridge into build/)."
    )


def build_appliance(
    recipe_path: str,
    output_path: Optional[str] = None,
    compositor_override: Optional[str] = None,
) -> str:
    """Compile an appliance recipe into an immutable EROFS cartridge image rootlessly."""
    recipe_path = os.path.abspath(recipe_path)
    if not os.path.isfile(recipe_path):
        raise FileNotFoundError(f"Recipe not found: {recipe_path}")

    print(f"[kurogane build] Loading recipe: {os.path.basename(recipe_path)}")
    data = yaml.load(recipe_path)
    manifest = schema.validate_manifest(data)

    app_name = manifest["appliance"]["name"]
    engine = manifest["runtime"]["engine"]
    display_entry = manifest["display"]["entrypoint"]
    args = manifest["display"].get("args", [])
    packages = manifest["runtime"].get("packages", [])

    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    build_dir = os.path.join(repo_root, "build")
    cache_dir = os.path.join(build_dir, "cache")
    stages_dir = os.path.join(repo_root, "stages")

    if not output_path:
        output_path = os.path.join(build_dir, f"cartridge_{app_name}_{engine}.img")
    else:
        output_path = os.path.abspath(output_path)

    os.makedirs(os.path.dirname(output_path), exist_ok=True)
    os.makedirs(cache_dir, exist_ok=True)

    print("=" * 60)
    print(f"[kurogane build] Compiling: {app_name} v{manifest['appliance']['version']}")
    print(f"[kurogane build] Runtime: {engine} | Target: {os.path.basename(output_path)}")
    print(f"[kurogane build] Entrypoint: {display_entry} {' '.join(args)}")
    print("=" * 60)

    # Step 1: Locate base image and extract rootlessly
    base_requested = manifest["runtime"].get("base_image")
    base_img = find_base_image(engine, build_dir, base_requested)
    print(f"[1/5] Extracting base image ({os.path.basename(base_img)}) via fsck.erofs...")
    staging_dir = tempfile.mkdtemp(prefix=f"cartilage_build_{app_name}_")

    try:
        extract_cmd = ["fsck.erofs", f"--extract={staging_dir}", base_img]
        res = subprocess.run(extract_cmd, capture_output=True)
        if res.returncode != 0 and not os.path.exists(os.path.join(staging_dir, "usr")):
            raise RuntimeError(f"Base extraction failed: {res.stderr.decode()}")

        # Ensure user write permissions
        subprocess.run(["chmod", "-R", "u+w", staging_dir], check=True)

        # Step 2: Ingest packages
        print(f"[2/5] Ingesting declared packages: {packages}...")
        binary_needed = os.path.basename(display_entry)
        bin_path = os.path.join(staging_dir, "usr", "bin", binary_needed)
        if not os.path.exists(bin_path):
            print(f"[kurogane build] Binary '{binary_needed}' not in base. Checking packages...")
            pacman_conf = os.path.join(build_dir, "pacman.conf")
            db_dir = os.path.join(build_dir, "pacman_db")

            target_pkgs: List[str] = []
            if os.path.isfile(pacman_conf) and shutil.which("pacman"):
                for p in packages:
                    res = subprocess.run(
                        ["pacman", "--config", pacman_conf, "--dbpath", db_dir, "-Sp", "--print-format", "%f", p],
                        capture_output=True, text=True
                    )
                    if res.returncode != 0 and shutil.which("fakeroot"):
                        print(f"[kurogane build] Fetching '{p}' via fakeroot pacman...")
                        subprocess.run(
                            ["fakeroot", "pacman", "--config", pacman_conf, "--dbpath", db_dir, "--cachedir", cache_dir, "-Sw", "--noconfirm", p],
                            check=True,
                        )
                        res = subprocess.run(
                            ["pacman", "--config", pacman_conf, "--dbpath", db_dir, "-Sp", "--print-format", "%f", p],
                            capture_output=True, text=True
                        )

                    if res.returncode == 0:
                        missing = []
                        for line in res.stdout.splitlines():
                            f = line.strip()
                            if f:
                                pkg_path = os.path.join(cache_dir, f)
                                if not os.path.isfile(pkg_path):
                                    missing.append(f)
                                elif pkg_path not in target_pkgs:
                                    target_pkgs.append(pkg_path)
                        if missing and shutil.which("fakeroot"):
                            print(f"[kurogane build] Fetching {len(missing)} missing packages for '{p}'...")
                            subprocess.run(
                                ["fakeroot", "pacman", "--config", pacman_conf, "--dbpath", db_dir, "--cachedir", cache_dir, "-Sw", "--noconfirm", p],
                                check=True,
                            )
                            for f in missing:
                                pkg_path = os.path.join(cache_dir, f)
                                if os.path.isfile(pkg_path) and pkg_path not in target_pkgs:
                                    target_pkgs.append(pkg_path)

            if not target_pkgs:
                target_pkgs = glob.glob(os.path.join(cache_dir, "*.pkg.tar.*"))

            print(f"[kurogane build] Unpacking {len(target_pkgs)} packages into appliance rootfs...")
            for pkg in target_pkgs:
                tar_cmd = ["tar"]
                if pkg.endswith(".zst"):
                    tar_cmd.append("--zstd")
                elif pkg.endswith(".xz"):
                    tar_cmd.append("--xz")
                tar_cmd.extend(["-xf", pkg, "-C", staging_dir, "--exclude=.PKGINFO", "--exclude=.BUILDINFO", "--exclude=.MTREE", "--exclude=.INSTALL"])
                subprocess.run(tar_cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        # Configure target Wayland compositor
        compositor = compositor_override or manifest["display"].get("compositor", "cage")
        if compositor == "cage":
            print(f"[kurogane build] Compositor: 'cage' (Dedicated Single-App Kiosk)")
            if "session" in display_entry.lower() or ("foot" in packages and "dillo" in packages):
                print(f"[kurogane build] [WARN] 'cage' runs strictly in single-window kiosk mode. For multi-app workflows, choose 'dwl' or 'sway'.")
        elif compositor == "dwl":
            print(f"[kurogane build] Compositor: 'dwl' (Ultra-Lean C-Based Dynamic Tiling)")
            dwl_candidates = [
                os.path.join(build_dir, "dwl"),
                os.path.expanduser("~/.local/bin/dwl"),
                shutil.which("dwl") or "",
            ]
            dwl_found = False
            for cand in dwl_candidates:
                if cand and os.path.isfile(cand):
                    dest = os.path.join(staging_dir, "usr", "bin", "dwl")
                    shutil.copy2(cand, dest)
                    os.chmod(dest, 0o755)
                    dwl_found = True
                    print(f"[kurogane build] Installed custom dwl compositor from {cand}")
                    break
            if not dwl_found:
                raise FileNotFoundError("dwl compositor binary not found in build/dwl or ~/.local/bin/dwl")

            # Ensure wlroots0.20 and dependencies are unpacked
            for dep_pkg in ["wlroots0.20", "libliftoff", "libdisplay-info"]:
                for match in glob.glob(os.path.join(cache_dir, f"{dep_pkg}*.pkg.tar.*")):
                    subprocess.run(
                        ["tar", "--zstd", "-xf", match, "-C", staging_dir, "--exclude=.PKGINFO", "--exclude=.BUILDINFO", "--exclude=.MTREE", "--exclude=.INSTALL"],
                        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
                    )
        elif compositor == "labwc":
            print(f"[kurogane build] Compositor: 'labwc' (Stacking/Floating Window Manager)")
            # Ensure labwc and its dependencies are unpacked
            for dep_pkg in ["labwc", "wlroots0.20", "cairo", "pango", "libinput", "libsfdo", "libxml2", "librsvg", "libpng"]:
                for match in glob.glob(os.path.join(cache_dir, f"{dep_pkg}*.pkg.tar.*")):
                    subprocess.run(
                        ["tar", "--zstd", "-xf", match, "-C", staging_dir, "--exclude=.PKGINFO", "--exclude=.BUILDINFO", "--exclude=.MTREE", "--exclude=.INSTALL"],
                        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
                    )
            # Install Kurogane custom labwc configuration
            labwc_cfg_dir = os.path.join(staging_dir, "etc", "xdg", "labwc")
            os.makedirs(labwc_cfg_dir, exist_ok=True)
            # 1. autostart: startup Foot terminal and browser
            with open(os.path.join(labwc_cfg_dir, "autostart"), "w", encoding="utf-8") as f:
                f.write("#!/bin/sh\nfoot &\n/usr/bin/cartilage-browser &\n")
            os.chmod(os.path.join(labwc_cfg_dir, "autostart"), 0o755)
            # 2. menu.xml (Right-click desktop Openbox menu)
            menu_src = os.path.join(stages_dir, "labwc-menu.xml")
            if os.path.isfile(menu_src):
                shutil.copy2(menu_src, os.path.join(labwc_cfg_dir, "menu.xml"))
                os.chmod(os.path.join(labwc_cfg_dir, "menu.xml"), 0o644)
            # 3. rc.xml (Keybindings, window buttons, mouse root context)
            rc_src = os.path.join(stages_dir, "labwc-rc.xml")
            if os.path.isfile(rc_src):
                shutil.copy2(rc_src, os.path.join(labwc_cfg_dir, "rc.xml"))
                os.chmod(os.path.join(labwc_cfg_dir, "rc.xml"), 0o644)
            # Also copy to user home directory ~/.config/labwc for runtime overrides
            user_labwc_dir = os.path.join(staging_dir, "home", "cartilage", ".config", "labwc")
            os.makedirs(user_labwc_dir, exist_ok=True)
            for f_name in ["autostart", "menu.xml", "rc.xml"]:
                src_f = os.path.join(labwc_cfg_dir, f_name)
                if os.path.isfile(src_f):
                    shutil.copy2(src_f, os.path.join(user_labwc_dir, f_name))
            print("[kurogane build] Installed Kurogane Stacking Desktop configuration (/etc/xdg/labwc)")
            if display_entry in ("/usr/bin/labwc", "/usr/bin/workstation-session") or not os.path.exists(os.path.join(staging_dir, display_entry.lstrip("/"))):
                display_entry = "/usr/bin/labwc"
        elif compositor == "sway":
            print(f"[kurogane build] Compositor: 'sway' (i3-Compatible Tiling Window Manager)")
            # Ensure sway package and its dependencies are unpacked
            for dep_pkg in ["sway", "wlroots0.20", "cairo", "pango", "libinput", "xcb-util-wm"]:
                for match in glob.glob(os.path.join(cache_dir, f"{dep_pkg}*.pkg.tar.*")):
                    subprocess.run(
                        ["tar", "--zstd", "-xf", match, "-C", staging_dir, "--exclude=.PKGINFO", "--exclude=.BUILDINFO", "--exclude=.MTREE", "--exclude=.INSTALL"],
                        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
                    )
            # Install Kurogane custom sway configuration
            sway_cfg_src = os.path.join(stages_dir, "sway-config")
            sway_cfg_dir = os.path.join(staging_dir, "etc", "cartilage")
            os.makedirs(sway_cfg_dir, exist_ok=True)
            sway_cfg_dest = os.path.join(sway_cfg_dir, "sway.conf")
            if os.path.isfile(sway_cfg_src):
                shutil.copy2(sway_cfg_src, sway_cfg_dest)
                os.chmod(sway_cfg_dest, 0o644)
                print(f"[kurogane build] Installed Kurogane i3/sway configuration to /etc/cartilage/sway.conf")
            # If display_entry is sway or workstation-session, ensure entrypoint points to sway
            if display_entry in ("/usr/bin/sway", "/usr/bin/workstation-session") or not os.path.exists(os.path.join(staging_dir, display_entry.lstrip("/"))):
                display_entry = "/usr/bin/sway"

        # Step 3: Inject Rust kurogane-init
        print("[3/5] Compiling and Installing Rust kurogane-init as /init...")
        
        init_crate = os.path.join(repo_root, "crates", "kurogane-init")
        subprocess.run(["cargo", "build", "--release"], cwd=init_crate, check=True)
        
        compiled_bin = os.path.join(init_crate, "target", "release", "kurogane-init")
        shutil.copy2(compiled_bin, os.path.join(staging_dir, "init"))
        os.chmod(os.path.join(staging_dir, "init"), 0o755)

        session_script = os.path.join(stages_dir, "workstation-session")
        if os.path.isfile(session_script):
            dest_session = os.path.join(staging_dir, "usr", "bin", "workstation-session")
            shutil.copy2(session_script, dest_session)
            os.chmod(dest_session, 0o755)

        # Note: We skip copying *.sh stages since we're using the monolithic Rust Micro-Supervisor

        # Step 4: Configure appliance entrypoint and system symlinks
        print("[4/5] Baking configuration & system symlinks...")
        cfg_dir = os.path.join(staging_dir, "etc", "cartilage")
        os.makedirs(cfg_dir, exist_ok=True)
        with open(os.path.join(cfg_dir, "entrypoint"), "w", encoding="utf-8") as f:
            f.write(display_entry + "\n")
        with open(os.path.join(cfg_dir, "compositor"), "w", encoding="utf-8") as f:
            f.write(compositor + "\n")
        with open(os.path.join(cfg_dir, "args"), "w", encoding="utf-8") as f:
            f.write("\n".join(args) + ("\n" if args else ""))
        disp_mode = manifest.get("display", {}).get("mode", "desktop")
        with open(os.path.join(cfg_dir, "mode"), "w", encoding="utf-8") as f:
            f.write(disp_mode + "\n")
        quota = manifest.get("storage", {}).get("quota", "256M")
        with open(os.path.join(cfg_dir, "quota"), "w", encoding="utf-8") as f:
            f.write(quota + "\n")
        accel_mode = manifest.get("hardware", {}).get("acceleration", "auto")
        with open(os.path.join(cfg_dir, "acceleration"), "w", encoding="utf-8") as f:
            f.write(accel_mode + "\n")
        env_vars = manifest["runtime"].get("environment", {})
        if env_vars:
            with open(os.path.join(cfg_dir, "env"), "w", encoding="utf-8") as f:
                for k, v in env_vars.items():
                    f.write(f"{k}={v}\n")

        # Ensure /etc/resolv.conf and /etc/asound.conf point to /run
        for conf_name in ["resolv.conf", "asound.conf"]:
            link_path = os.path.join(staging_dir, "etc", conf_name)
            try:
                if os.path.islink(link_path) or os.path.exists(link_path):
                    os.unlink(link_path)
                os.symlink(f"/run/{conf_name}", link_path)
            except Exception:
                pass

        # Protect resolv.conf in dhcpcd.conf
        dhcpcd_conf = os.path.join(staging_dir, "etc", "dhcpcd.conf")
        try:
            with open(dhcpcd_conf, "a", encoding="utf-8") as f:
                f.write("\nnohook resolv.conf\n")
        except Exception:
            pass

        # Bake cartilage user (UID 1000) and required groups into /etc/passwd and /etc/group
        etc_dir = os.path.join(staging_dir, "etc")
        os.makedirs(etc_dir, exist_ok=True)
        passwd_file = os.path.join(etc_dir, "passwd")
        group_file = os.path.join(etc_dir, "group")
        shadow_file = os.path.join(etc_dir, "shadow")

        user_shell = "/bin/bash"
        if not os.path.exists(os.path.join(staging_dir, "bin", "bash")) and not os.path.exists(os.path.join(staging_dir, "usr", "bin", "bash")):
            user_shell = "/bin/sh"
        cartilage_entry = f"cartilage:x:1000:1000:Kurogane User:/home/cartilage:{user_shell}\n"
        if os.path.isfile(passwd_file):
            with open(passwd_file, "r+", encoding="utf-8", errors="ignore") as f:
                content = f.read()
                lines = content.splitlines()
                found = False
                new_lines = []
                for line in lines:
                    if line.startswith("cartilage:"):
                        parts = line.split(":")
                        if len(parts) >= 7:
                            parts[6] = user_shell
                            line = ":".join(parts)
                        found = True
                    new_lines.append(line)
                if not found:
                    new_lines.append(cartilage_entry.strip())
                f.seek(0)
                f.truncate()
                f.write("\n".join(new_lines) + "\n")
        else:
            with open(passwd_file, "w", encoding="utf-8") as f:
                f.write(f"root:x:0:0::/root:{user_shell}\n" + cartilage_entry)

        # Inject zero-dependency Kurogane fastfetch banner
        usr_bin = os.path.join(staging_dir, "usr", "bin")
        os.makedirs(usr_bin, exist_ok=True)
        fastfetch_path = os.path.join(usr_bin, "fastfetch")
        fastfetch_content = """#!/bin/bash
CYAN='\\033[0;36m'
BLUE='\\033[0;34m'
GREEN='\\033[0;32m'
BOLD='\\033[1m'
NC='\\033[0m'

UPTIME=$(cat /proc/uptime 2>/dev/null | cut -d' ' -f1)
MEM_TOTAL=$(free -h 2>/dev/null | awk '/^Mem:/ {print $2}')
MEM_USED=$(free -h 2>/dev/null | awk '/^Mem:/ {print $3}')
MEM_AVAIL=$(free -h 2>/dev/null | awk '/^Mem:/ {print $NF}')
COMP=$(cat /etc/cartilage/compositor 2>/dev/null || echo "Wayland")

cat << 'EOF'
  ____           _   _ _                  ___  ____  
 / ___|__ _ _ __| |_(_) | __ _  __ _  ___/ _ \\/ ___| 
| |   / _` | '__| __| | |/ _` |/ _` |/ _ \\ | | \\___ \\ 
| |__| (_| | |  | |_| | | (_| | (_| |  __/ |_| |___) |
 \\____\\__,_|_|   \\__|_|_|\\__,_|\\__, |\\___|\\___/|____/ 
                               |___/                  
EOF
echo -e "${CYAN}${BOLD}OS:${NC}         Kurogane OS (Immutable EROFS Appliance)"
echo -e "${CYAN}${BOLD}Kernel:${NC}     $(uname -r) ($(uname -m))"
echo -e "${CYAN}${BOLD}Compositor:${NC} ${COMP} (Direct DRM/KMS)"
echo -e "${CYAN}${BOLD}Uptime:${NC}     ${UPTIME}s (Cold Boot <2s)"
echo -e "${CYAN}${BOLD}Memory:${NC}     ${MEM_USED} / ${MEM_TOTAL} (Available: ${MEM_AVAIL})"
echo -e "${CYAN}${BOLD}Storage:${NC}    100% Read-Only EROFS + /data (Persistent ext4)"
echo -e "${CYAN}${BOLD}Shell:${NC}      ${SHELL:-/bin/bash}"
echo ""
"""
        with open(fastfetch_path, "w", encoding="utf-8") as f:
            f.write(fastfetch_content)
        os.chmod(fastfetch_path, 0o755)
        cartilage_fetch = os.path.join(usr_bin, "cartilage-fetch")
        if not os.path.exists(cartilage_fetch):
            try:
                os.symlink("fastfetch", cartilage_fetch)
            except Exception:
                pass

        # Inject universal application wrappers (cartilage-browser and cartilage-media)
        browser_wrapper_path = os.path.join(usr_bin, "cartilage-browser")
        browser_wrapper = """#!/bin/bash
if [[ -x /usr/bin/chromium ]]; then
    DATA_DIR="/data/chromium"
    [[ ! -d /data ]] && DATA_DIR="/tmp/chromium"
    mkdir -p "$DATA_DIR" 2>/dev/null || true
    CHROMIUM_FLAGS=(
        "--ozone-platform=wayland"
        "--enable-features=UseOzonePlatform"
        "--disable-features=AudioServiceSandbox"
        "--disable-quic"
        "--disable-accelerated-video-decode"
        "--alsa-output-device=default"
        "--alsa-input-device=default"
        "--autoplay-policy=no-user-gesture-required"
        "--no-proxy-server"
        "--no-first-run"
        "--no-default-browser-check"
        "--user-data-dir=$DATA_DIR"
    )

    # Check if 3D acceleration is enabled via VirGL or hardware GPU
    if grep -q "cartilage_virgl=1" /proc/cmdline 2>/dev/null || [[ -e /dev/dri/renderD128 && ! -e /sys/module/virtio_gpu ]]; then
        CHROMIUM_FLAGS+=(
            "--enable-gpu-rasterization"
            "--ignore-gpu-blocklist"
        )
    else
        # Stable software compositing for 2D QEMU displays:
        # Prevents DRM_IOCTL_MODE_CREATE_DUMB permission crashes on /dev/dri/card0
        # Multithreaded software decoders (dav1d AV1 / libvpx VP9) play YouTube smoothly via Wayland wl_shm
        CHROMIUM_FLAGS+=(
            "--disable-gpu"
            "--disable-gpu-watchdog"
        )
    fi

    exec /usr/bin/chromium "${CHROMIUM_FLAGS[@]}" "${@:-https://youtube.com}"
elif [[ -x /usr/bin/firefox ]]; then
    exec /usr/bin/firefox "${@:-https://youtube.com}"
elif [[ -x /usr/bin/dillo ]]; then
    exec /usr/bin/dillo "${@:-https://duckduckgo.com}"
fi
"""
        with open(browser_wrapper_path, "w", encoding="utf-8") as f:
            f.write(browser_wrapper)
        os.chmod(browser_wrapper_path, 0o755)

        media_wrapper_path = os.path.join(usr_bin, "cartilage-media")
        media_wrapper = """#!/bin/bash
if [[ -x /usr/bin/mpv ]]; then
    exec /usr/bin/mpv --player-operation-mode=pseudo-gui --idle=yes "$@"
elif [[ -x /usr/bin/vlc ]]; then
    exec /usr/bin/vlc "$@"
fi
"""
        with open(media_wrapper_path, "w", encoding="utf-8") as f:
            f.write(media_wrapper)
        os.chmod(media_wrapper_path, 0o755)

        # Inject dynamic live telemetry status script for swaybar
        status_path = os.path.join(usr_bin, "cartilage-status")
        status_content = """#!/bin/bash
while true; do
    if [[ -r /proc/meminfo ]]; then
        total_kb=$(awk '/MemTotal:/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)
        avail_kb=$(awk '/MemAvailable:/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)
        if [[ $total_kb -gt 0 ]]; then
            used_mb=$(( (total_kb - avail_kb) / 1024 ))
            total_mb=$(( total_kb / 1024 ))
            RAM_STR="${used_mb}M/${total_mb}M"
        else
            RAM_STR="0M/0M"
        fi
    else
        RAM_STR="0M/0M"
    fi
    TIME=$(date '+%H:%M' 2>/dev/null || echo "--:--")
    echo "🚀 CLICK BAR OR Alt+d: Menu | Alt+Enter: Term | Alt+w: Web | Alt+m: MPV | RAM: ${RAM_STR} | ${TIME}"
    sleep 5
done
"""
        with open(status_path, "w", encoding="utf-8") as f:
            f.write(status_content)
        os.chmod(status_path, 0o755)

        # Compile setuid root sudo helper for unprivileged user cartilage
        sudo_c_src = """#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    if (setgid(0) != 0 || setuid(0) != 0) {
        perror("sudo: failed to elevate privileges");
        return 1;
    }
    if (argc == 1 || (argc == 2 && (strcmp(argv[1], "-s") == 0 || strcmp(argv[1], "-i") == 0))) {
        char *sh = "/bin/bash";
        execl(sh, sh, NULL);
        perror("sudo: exec /bin/bash");
        return 1;
    }
    int offset = 1;
    if (strcmp(argv[1], "-s") == 0 || strcmp(argv[1], "-i") == 0) {
        offset = 2;
    }
    if (offset >= argc) {
        char *sh = "/bin/bash";
        execl(sh, sh, NULL);
        return 0;
    }
    execvp(argv[offset], argv + offset);
    perror("sudo: execvp");
    return 1;
}
"""
        sudo_c_path = os.path.join(tempfile.gettempdir(), f"cartilage_sudo_{os.getpid()}.c")
        sudo_bin_path = os.path.join(usr_bin, "sudo")
        try:
            with open(sudo_c_path, "w", encoding="utf-8") as f:
                f.write(sudo_c_src)
            subprocess.run(["gcc", "-O2", "-Wall", sudo_c_path, "-o", sudo_bin_path], check=True)
            os.chmod(sudo_bin_path, 0o4755)
            bin_dir = os.path.join(staging_dir, "bin")
            if os.path.isdir(bin_dir) and not os.path.exists(os.path.join(bin_dir, "sudo")):
                try:
                    os.symlink("/usr/bin/sudo", os.path.join(bin_dir, "sudo"))
                except Exception:
                    pass
            print("[kurogane build] Compiled setuid /usr/bin/sudo helper")
        except Exception as e:
            print(f"[kurogane build] [WARN] Failed to compile setuid sudo: {e}")
        finally:
            if os.path.exists(sudo_c_path):
                os.remove(sudo_c_path)

        # Inject cartilage-unlock utility for writable overlay package installation
        unlock_path = os.path.join(usr_bin, "cartilage-unlock")
        unlock_content = """#!/bin/bash
if [[ $(id -u) -ne 0 ]]; then
    echo "Error: cartilage-unlock must be run as root (run 'sudo cartilage-unlock')" >&2
    exit 1
fi
mkdir -p /run/overlay_usr/{upper,work} /run/overlay_etc/{upper,work} /run/overlay_var/{upper,work}
mount -t overlay overlay -o lowerdir=/usr,upperdir=/run/overlay_usr/upper,workdir=/run/overlay_usr/work /usr 2>/dev/null || true
mount -t overlay overlay -o lowerdir=/etc,upperdir=/run/overlay_etc/upper,workdir=/run/overlay_etc/work /etc 2>/dev/null || true
mount -t overlay overlay -o lowerdir=/var,upperdir=/run/overlay_var/upper,workdir=/run/overlay_var/work /var 2>/dev/null || true
echo "[kurogane] Filesystem unlocked with writable RAM OverlayFS on /usr, /etc, /var."
echo "[kurogane] You can now run 'pacman -Sy <package>' to install tools in this live session."
"""
        with open(unlock_path, "w", encoding="utf-8") as f:
            f.write(unlock_content)
        os.chmod(unlock_path, 0o755)

        # Inject Kurogane Appliance Package Guard enforcing the "Bake, Don't Mutate" principle
        pacman_bin = os.path.join(usr_bin, "pacman")
        pacman_real = os.path.join(usr_bin, "pacman.real")
        if os.path.isfile(pacman_bin) and not os.path.isfile(pacman_real):
            os.rename(pacman_bin, pacman_real)
            pacman_wrapper = """#!/bin/bash
# Kurogane OS - Appliance Package Guard
# Enforces the "Bake, Don't Mutate" immutable appliance principle.

REAL_PACMAN="/usr/bin/pacman.real"

# Allow non-mutating queries / version checks
if [[ "$1" =~ ^-[QVh] || "$1" == "--version" || "$1" == "--help" ]]; then
    if [[ -x "$REAL_PACMAN" ]]; then
        exec "$REAL_PACMAN" "$@"
    fi
fi

# Check for explicit ephemeral testing override
FORCE_EPHEMERAL=0
FILTERED_ARGS=()
for arg in "$@"; do
    if [[ "$arg" == "--force-ephemeral" ]]; then
        FORCE_EPHEMERAL=1
    else
        FILTERED_ARGS+=("$arg")
    fi
done

if [[ $FORCE_EPHEMERAL -eq 1 && -x "$REAL_PACMAN" ]]; then
    echo "[kurogane] WARNING: Running pacman in temporary RAM OverlayFS. Changes will disappear on reboot." >&2
    exec "$REAL_PACMAN" --overwrite '*' "${FILTERED_ARGS[@]}"
fi

cat << 'NOTICE'
======================================================================
                 KUROGANE OS — IMMUTABLE APPLIANCE
======================================================================
 This cartridge is an immutable read-only appliance.
 Direct runtime package installation is intentionally disabled:
   * Files installed to /usr vanish completely upon reboot.
   * Downloading packages into RAM tmpfs exhausts system memory.

 [HOW TO ADD PACKAGES PERMANENTLY]
 1. Add desired packages to your recipe YAML on your host:
      runtime:
        packages:
          - <package-name>
 2. Rebuild the immutable cartridge:
      ./kurogane build recipes/<recipe>.yaml

 [PERSISTENT USER STORAGE]
 User code, dotfiles, Git repos, and notes persist safely in:
      /data  (or ~ when persistent CARTDATA drive is attached)

 (To bypass for temporary live debugging: pacman --force-ephemeral ...)
======================================================================
NOTICE
exit 1
"""
            with open(pacman_bin, "w", encoding="utf-8") as f:
                f.write(pacman_wrapper)
            os.chmod(pacman_bin, 0o755)
            
            # Also install in /usr/local/bin/pacman for PATH priority
            usr_local_bin = os.path.join(staging_dir, "usr", "local", "bin")
            os.makedirs(usr_local_bin, exist_ok=True)
            with open(os.path.join(usr_local_bin, "pacman"), "w", encoding="utf-8") as f:
                f.write(pacman_wrapper)
            os.chmod(os.path.join(usr_local_bin, "pacman"), 0o755)
            print("[kurogane build] Injected Kurogane Appliance Package Guard for pacman")

        # Inject cartilage utilities (menu, stress, persistent term, anti-void)
        for util_name in ["cartilage-menu", "cartilage-stress", "cartilage-session-term", "cartilage-anti-void"]:
            src_util = os.path.join(stages_dir, util_name)
            if os.path.isfile(src_util):
                shutil.copy2(src_util, os.path.join(usr_bin, util_name))
                os.chmod(os.path.join(usr_bin, util_name), 0o755)

        # Inject Tokyo Night styling for foot terminal
        foot_dir = os.path.join(staging_dir, "etc", "xdg", "foot")
        os.makedirs(foot_dir, exist_ok=True)
        foot_ini = os.path.join(foot_dir, "foot.ini")
        foot_config = """[main]
font=monospace:size=11
pad=12x12
shell=/bin/bash

[cursor]
style=block
blink=yes

[colors-dark]
alpha=0.95
background=1a1b26
foreground=c0caf5
regular0=15161e
regular1=f7768e
regular2=9ece6a
regular3=e0af68
regular4=7aa2f7
regular5=bb9af7
regular6=7dcfff
regular7=a9b1d6
bright0=414868
bright1=f7768e
bright2=9ece6a
bright3=e0af68
bright4=7aa2f7
bright5=bb9af7
bright6=7dcfff
bright7=c0caf5
"""
        with open(foot_ini, "w", encoding="utf-8") as f:
            f.write(foot_config)

        # Inject user .bashrc with colorful prompt and auto-fastfetch
        home_dir = os.path.join(staging_dir, "home", "cartilage")
        os.makedirs(home_dir, exist_ok=True)
        bashrc_path = os.path.join(home_dir, ".bashrc")
        bashrc_content = """# Kurogane OS user environment
export PS1='\\[\\033[01;34m\\]cartilage@os\\[\\033[00m\\]:\\[\\033[01;36m\\]\\w\\[\\033[00m\\]\\$ '
export TERM=foot
alias ll='ls -la --color=auto'
alias ls='ls --color=auto'
alias fastfetch='/usr/bin/fastfetch'
alias chromium='/usr/bin/cartilage-browser'
alias browser='/usr/bin/cartilage-browser'
alias menu='/usr/bin/cartilage-menu'
alias stress='/usr/bin/cartilage-stress'
alias unlock='sudo cartilage-unlock'
alias pacman='pacman --overwrite "*"'

# Greet user if interactive shell
if [[ $- == *i* ]]; then
    /usr/bin/fastfetch
fi
"""
        with open(bashrc_path, "w", encoding="utf-8") as f:
            f.write(bashrc_content)

        req_groups = {
            "cartilage": "1000",
            "audio": "995",
            "video": "983",
            "input": "992",
            "seat": "969",
        }
        group_lines = []
        existing_groups = set()
        if os.path.isfile(group_file):
            with open(group_file, "r", encoding="utf-8", errors="ignore") as f:
                for line in f:
                    parts = line.strip().split(":")
                    if parts and parts[0]:
                        existing_groups.add(parts[0])
                        if parts[0] in req_groups and "cartilage" not in parts[-1].split(","):
                            members = [m for m in parts[-1].split(",") if m]
                            members.append("cartilage")
                            line = f"{parts[0]}:{parts[1]}:{parts[2]}:{','.join(members)}\n"
                    group_lines.append(line)

        for grp, gid in req_groups.items():
            if grp not in existing_groups:
                members = "cartilage" if grp != "cartilage" else ""
                group_lines.append(f"{grp}:x:{gid}:{members}\n")

        with open(group_file, "w", encoding="utf-8") as f:
            f.writelines(group_lines)

        if os.path.isfile(shadow_file):
            with open(shadow_file, "r+", encoding="utf-8", errors="ignore") as f:
                content = f.read()
                if "cartilage:" not in content:
                    f.write("cartilage:*:19000:0:99999:7:::\n")

        # Ensure home directory structure
        os.makedirs(os.path.join(staging_dir, "home", "cartilage"), exist_ok=True)

        # Sanitization pass: remove manuals and documentation
        for doc_dir in [os.path.join(staging_dir, "usr", "share", "doc"), os.path.join(staging_dir, "usr", "share", "man")]:
            shutil.rmtree(doc_dir, ignore_errors=True)

        # Ensure user has full read/write/traverse access across all unpacked tree
        subprocess.run(["chmod", "-R", "u+rwX", staging_dir], check=True)

        # Ensure setuid bit is preserved on /usr/bin/sudo
        sudo_final = os.path.join(usr_bin, "sudo")
        if os.path.exists(sudo_final):
            os.chmod(sudo_final, 0o4755)

        # Step 5: Compile EROFS
        print(f"[5/5] Compiling immutable EROFS image: {os.path.basename(output_path)}...")
        temp_out = output_path + ".tmp"
        if os.path.exists(temp_out):
            os.remove(temp_out)
        mkfs_cmd = ["mkfs.erofs", "--all-root", "-zlz4hc,12", temp_out, staging_dir]
        subprocess.run(mkfs_cmd, check=True, stdout=subprocess.DEVNULL)
        os.replace(temp_out, output_path)

        # Step 6: Cryptographic dm-verity Merkle Tree Computation
        verity_meta = compute_verity_merkle_tree(output_path)
        print(f"[kurogane build] [VERITY] dm-verity SHA-256 root hash: {verity_meta['root_hash']}")

        size_mb = os.path.getsize(output_path) // (1024 * 1024)
        print(f"[SUCCESS] Cartridge compiled successfully: {output_path} ({size_mb} MiB)")
        return output_path

    finally:
        subprocess.run(["chmod", "-R", "u+w", staging_dir], stderr=subprocess.DEVNULL)
        shutil.rmtree(staging_dir, ignore_errors=True)


def compute_verity_merkle_tree(image_path: str, block_size: int = 4096, salt_hex: str = "") -> Dict[str, Any]:
    """
    Computes a cryptographic dm-verity SHA-256 Merkle tree for a block image.
    Divides data into 4096-byte blocks and produces the root hash and metadata manifest.
    """
    salt = bytes.fromhex(salt_hex) if salt_hex else b""
    file_size = os.path.getsize(image_path)
    data_blocks = (file_size + block_size - 1) // block_size

    current_layer_hashes: List[bytes] = []
    with open(image_path, "rb") as f:
        for _ in range(data_blocks):
            chunk = f.read(block_size)
            if len(chunk) < block_size:
                chunk = chunk.ljust(block_size, b"\x00")
            h = hashlib.sha256(salt + chunk).digest()
            current_layer_hashes.append(h)

    if not current_layer_hashes:
        return {"root_hash": hashlib.sha256().hexdigest(), "data_blocks": 0, "salt": salt_hex}

    # Build Merkle layers until a single root hash remains
    while len(current_layer_hashes) > 1:
        next_layer: List[bytes] = []
        hashes_per_block = block_size // 32  # 128 SHA-256 hashes per 4KB block
        for i in range(0, len(current_layer_hashes), hashes_per_block):
            block_hashes = current_layer_hashes[i : i + hashes_per_block]
            packed_block = b"".join(block_hashes)
            if len(packed_block) < block_size:
                packed_block = packed_block.ljust(block_size, b"\x00")
            next_layer.append(hashlib.sha256(salt + packed_block).digest())
        current_layer_hashes = next_layer

    root_hash = current_layer_hashes[0].hex()
    meta = {
        "root_hash": root_hash,
        "data_blocks": data_blocks,
        "data_block_size": block_size,
        "hash_block_size": block_size,
        "salt": salt_hex,
    }
    manifest_path = image_path + ".verity.json"
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(meta, f, indent=2)
    return meta


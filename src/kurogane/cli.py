"""
KUROGANE (黒鉄) CLI Entry Point.
Arcade boards for operating systems.
Provides unified subcommands: validate, build, run, compose, flash.
"""

import argparse
import os
import sys
from typing import List, Optional

from . import builder, composer, flasher, runner, schema, yaml


def cmd_validate(args: argparse.Namespace) -> int:
    """Validate one or more appliance recipe manifests against schema."""
    success = True
    for path in args.recipes:
        try:
            print(f"[validate] Checking {path}...")
            data = yaml.load(path)
            schema.validate_manifest(data)
            print(f"[validate] [PASS] {path} is valid.")
        except Exception as e:
            print(f"[validate] [FAIL] {path}: {e}", file=sys.stderr)
            success = False
    return 0 if success else 1


def cmd_build(args: argparse.Namespace) -> int:
    """Build an arcade board appliance from a recipe."""
    try:
        builder.build_appliance(args.recipe, args.output, compositor_override=getattr(args, "compositor", None))
        return 0
    except Exception as e:
        print(f"[kurogane build] Error: {e}", file=sys.stderr)
        return 1


def cmd_run(args: argparse.Namespace) -> int:
    """Run an appliance in QEMU from a recipe or arcade board image."""
    try:
        return runner.run_appliance(
            target=args.target,
            test_mode=args.test,
            url=args.url,
            extra_cmdline=args.append,
            data_image=args.data,
            efi_mode=args.efi,
            verbose=args.verbose,
            compositor=getattr(args, "compositor", None),
            accel=getattr(args, "accel", False),
            force_software_render=getattr(args, "force_software_render", False),
            verify_verity=getattr(args, "verify_verity", False),
            enforce_seals=getattr(args, "enforce_seals", False),
        )
    except Exception as e:
        print(f"[kurogane run] Error: {e}", file=sys.stderr)
        return 1


def cmd_compose(args: argparse.Namespace) -> int:
    """Compose multiple arcade boards into a multi-boot UEFI GPT disk image."""
    try:
        if getattr(args, "hub", False):
            composer.compose_hub_disk(args.output, args.targets)
        else:
            composer.compose_disk(args.output, args.targets)
        return 0
    except Exception as e:
        print(f"[kurogane compose] Error: {e}", file=sys.stderr)
        return 1


def cmd_flash(args: argparse.Namespace) -> int:
    """Safely flash one or more arcade boards to a USB block device."""
    target_device = args.target
    recipes = list(args.args)
    if not target_device:
        target_device = recipes.pop(0)
    if not recipes:
        print("[kurogane flash] Error: At least one recipe or arcade board image is required.", file=sys.stderr)
        return 1
    try:
        return flasher.flash_device(
            target_device=target_device,
            recipes=recipes,
            dry_run=args.dry_run,
            force_internal=getattr(args, "force_internal", False),
        )
    except Exception as e:
        print(f"[kurogane flash] Error: {e}", file=sys.stderr)
        return 1


def cmd_init_hub(args: argparse.Namespace) -> int:
    """Format and initialize a USB drive as a Dynamic Hub (Mode 2)."""
    target = args.target
    try:
        return flasher.init_hub(
            target_device=target,
            dry_run=args.dry_run,
            force_internal=args.force_internal,
        )
    except Exception as e:
        print(f"[kurogane init-hub] Error: {e}", file=sys.stderr)
        return 1


def cmd_test_golden(args: argparse.Namespace) -> int:
    """Run Golden Master 7-pillar appliance verification in QEMU."""
    print("=" * 60)
    print(f"[kurogane] Running Golden Master Verification: {args.target}")
    print("=" * 60)
    return runner.run_appliance(
        target=args.target,
        test_mode=True,
        extra_cmdline="kurogane_test=golden cartilage_test=golden",
        verbose=True,
    )


def cmd_stress(args: argparse.Namespace) -> int:
    """Run torture stress testing against appliance in QEMU."""
    duration = getattr(args, "duration", 10) or 10
    print("=" * 60)
    print(f"[kurogane] Launching Appliance Torture Stress Test ({duration}s): {args.target}")
    print("=" * 60)
    return runner.run_appliance(
        target=args.target,
        test_mode=True,
        extra_cmdline=f"kurogane_test=stress cartilage_test=stress kurogane_stress_duration={duration} cartilage_stress_duration={duration}",
        verbose=True,
    )


def main(argv: Optional[List[str]] = None) -> int:
    """Main CLI entrypoint parser."""
    parser = argparse.ArgumentParser(
        prog="kurogane",
        description="KUROGANE (黒鉄) — Arcade Boards for Operating Systems",
    )
    subparsers = parser.add_subparsers(dest="subcommand", help="Available subcommands")


    # 1. validate
    p_val = subparsers.add_parser("validate", help="Validate appliance recipe manifests against schema")
    p_val.add_argument("recipes", nargs="+", help="Path to one or more recipe YAML files")

    # 2. build
    p_build = subparsers.add_parser("build", help="Compile a recipe manifest into an EROFS arcade board")
    p_build.add_argument("recipe", help="Path to recipe YAML file")
    p_build.add_argument("-o", "--output", help="Output path for the compiled .img arcade board")
    p_build.add_argument("--compositor", choices=["cage", "labwc", "dwl", "sway", "none"], help="Override Wayland compositor declared in recipe")

    # 3. run
    p_run = subparsers.add_parser("run", help="Launch an arcade board appliance in QEMU")
    p_run.add_argument("target", help="Path to recipe YAML file or .img arcade board image")
    p_run.add_argument("--url", help="Initial URL passed to web browser appliances")
    p_run.add_argument("--compositor", choices=["cage", "labwc", "dwl", "sway", "none"], help="Override Wayland compositor at runtime")
    p_run.add_argument("--test", action="store_true", help="Run headlessly in automated test verification mode")
    p_run.add_argument("--data", help="Custom persistent CARTDATA disk image path")
    p_run.add_argument("--efi", action="store_true", help="Boot in UEFI mode")
    p_run.add_argument("--append", help="Extra kernel commandline parameters")
    p_run.add_argument("-v", "--verbose", action="store_true", help="Print guest serial console logs directly to terminal")
    p_run.add_argument("--accel", "--virgl", dest="accel", action="store_true", help="Enable hardware 3D graphics acceleration (VirGL) in QEMU")
    p_run.add_argument("--force-software-render", action="store_true", help="Force Pixman CPU software rendering fallback (WLR_RENDERER=pixman)")
    p_run.add_argument("--verify-verity", action="store_true", help="Enforce dm-verity Merkle tree root hash verification in kernel command line")
    p_run.add_argument("--enforce-seals", action="store_true", help="Enforce strict memory buffer sealing (reject unsealed memfds)")

    # 4. compose
    p_comp = subparsers.add_parser("compose", help="Compose multiple arcade boards into a multi-boot UEFI disk")
    p_comp.add_argument("-o", "--output", required=True, help="Output path for combined disk image")
    p_comp.add_argument("--hub", action="store_true", help="Compose a Mode 2 Dynamic Hub image with exFAT payload partition")
    p_comp.add_argument("targets", nargs="+", help="Paths to recipe YAMLs or arcade board .img files")

    # 5. flash
    p_flash = subparsers.add_parser("flash", help="Safely flash arcade boards to a physical USB drive (Mode 1 Dedicated)")
    p_flash.add_argument("--target", help="Destination block device (e.g. /dev/sdX)")
    p_flash.add_argument("--dry-run", action="store_true", help="Simulate layout calculation without writing blocks")
    p_flash.add_argument("--force-internal", action="store_true", help="Allow targeting internal drives (dangerous)")
    p_flash.add_argument("args", nargs="+", help="Destination block device (if not using --target) followed by recipe YAMLs or arcade board .img files")

    # 6. init-hub
    p_inithub = subparsers.add_parser("init-hub", help="Format and initialize a USB drive as a Dynamic Hub (Mode 2)")
    p_inithub.add_argument("target", help="Destination block device (e.g. /dev/sdX)")
    p_inithub.add_argument("--dry-run", action="store_true", help="Simulate layout calculation without writing blocks")
    p_inithub.add_argument("--force-internal", action="store_true", help="Allow targeting internal drives (dangerous)")

    # 7. test-golden
    p_golden = subparsers.add_parser("test-golden", help="Run 7-pillar Golden Master verification suite")
    p_golden.add_argument("target", help="Path to recipe YAML file or .img arcade board image")

    # 8. stress
    p_stress = subparsers.add_parser("stress", help="Run multi-threaded machine & kernel torture stress benchmark")
    p_stress.add_argument("target", help="Path to recipe YAML file or .img arcade board image")
    p_stress.add_argument("--duration", type=int, default=10, help="Duration of stress test in seconds (default: 10)")

    parsed = parser.parse_args(argv)
    if not parsed.subcommand:
        parser.print_help()
        return 0

    dispatch = {
        "validate": cmd_validate,
        "build": cmd_build,
        "run": cmd_run,
        "compose": cmd_compose,
        "flash": cmd_flash,
        "init-hub": cmd_init_hub,
        "test-golden": cmd_test_golden,
        "stress": cmd_stress,
    }

    try:
        return dispatch[parsed.subcommand](parsed)
    except BrokenPipeError:
        try:
            sys.stdout.close()
            sys.stderr.close()
        except Exception:
            pass
        return 0


if __name__ == "__main__":
    sys.exit(main())


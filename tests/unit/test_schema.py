"""
Unit tests for kurogane.schema.
Directly tests manifest validation, defaults population, and rejection invariants.
"""

import glob
import os
import pytest
import sys

# Ensure src is in sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../../src")))

from kurogane.schema import validate_manifest, ValidationError
from kurogane.yaml import load as yaml_load


def test_all_production_recipes_validate():
    """Verify all real production recipes in recipes/ validate cleanly with 0 errors."""
    recipe_files = glob.glob("recipes/*.yaml")
    assert len(recipe_files) > 0, "No recipes found in recipes/"
    for recipe_path in recipe_files:
        data = yaml_load(recipe_path)
        validated = validate_manifest(data)
        assert isinstance(validated, dict)
        assert "appliance" in validated
        assert "runtime" in validated
        assert "display" in validated
        assert "storage" in validated
        assert "hardware" in validated


def test_schema_validates_minimal_valid_manifest():
    manifest = {
        "appliance": {
            "name": "test-app",
            "version": "1.0.0",
        },
        "runtime": {
            "engine": "arch",
        },
        "display": {
            "entrypoint": "/bin/true",
        },
        "storage": {
            "mode": "persistent",
        },
        "hardware": {},
    }
    validated = validate_manifest(manifest)
    assert validated["appliance"]["name"] == "test-app"
    assert validated["runtime"]["engine"] == "arch"
    assert validated["runtime"]["packages"] == []
    assert validated["display"]["compositor"] == "cage"
    assert validated["display"]["mode"] == "desktop"
    assert validated["storage"]["mode"] == "persistent"
    assert validated["storage"]["mount_point"] == "/data"
    assert validated["hardware"]["memory"] == "1024M"
    assert validated["hardware"]["cores"] == 2
    assert validated["hardware"]["network"] is False
    assert validated["hardware"]["audio"] is False


def test_reject_non_dict_root():
    with pytest.raises(ValidationError, match="Manifest root must be a mapping/dictionary"):
        validate_manifest(["not", "a", "dict"])


def test_reject_missing_required_section():
    manifest = {
        "appliance": {"name": "app", "version": "1.0"},
        # missing runtime, display, storage, hardware
    }
    with pytest.raises(ValidationError, match="Missing required top-level section"):
        validate_manifest(manifest)


def test_reject_unknown_top_level_section():
    manifest = {
        "appliance": {"name": "app", "version": "1.0"},
        "runtime": {"engine": "arch"},
        "display": {"entrypoint": "/bin/true"},
        "storage": {"mode": "persistent"},
        "hardware": {},
        "unknown_section": {},
    }
    with pytest.raises(ValidationError, match="Unknown top-level property"):
        validate_manifest(manifest)


@pytest.mark.parametrize("invalid_name", [
    "",
    "   ",
    "App_With_Capital",
    "invalid@character",
    "spaces in name",
    "app.with.dots",
])
def test_reject_invalid_appliance_name(invalid_name):
    manifest = {
        "appliance": {"name": invalid_name, "version": "1.0"},
        "runtime": {"engine": "arch"},
        "display": {"entrypoint": "/bin/true"},
        "storage": {"mode": "persistent"},
        "hardware": {},
    }
    with pytest.raises(ValidationError):
        validate_manifest(manifest)


@pytest.mark.parametrize("invalid_engine", [
    "ubuntu",
    "debian",
    "fedora",
    "nixos",
    123,
])
def test_reject_invalid_engine(invalid_engine):
    manifest = {
        "appliance": {"name": "valid-app", "version": "1.0"},
        "runtime": {"engine": invalid_engine},
        "display": {"entrypoint": "/bin/true"},
        "storage": {"mode": "persistent"},
        "hardware": {},
    }
    with pytest.raises(ValidationError):
        validate_manifest(manifest)


@pytest.mark.parametrize("invalid_compositor", [
    "gnome",
    "kde",
    "xorg",
    "wayfire",
])
def test_reject_invalid_compositor(invalid_compositor):
    manifest = {
        "appliance": {"name": "valid-app", "version": "1.0"},
        "runtime": {"engine": "arch"},
        "display": {"entrypoint": "/bin/true", "compositor": invalid_compositor},
        "storage": {"mode": "persistent"},
        "hardware": {},
    }
    with pytest.raises(ValidationError, match="Invalid display.compositor"):
        validate_manifest(manifest)


@pytest.mark.parametrize("invalid_quota", [
    "256X",
    "-10M",
    "abc",
    "100GB_extra",
    "!!M",
])
def test_reject_invalid_quota(invalid_quota):
    manifest = {
        "appliance": {"name": "valid-app", "version": "1.0"},
        "runtime": {"engine": "arch"},
        "display": {"entrypoint": "/bin/true"},
        "storage": {"mode": "persistent", "quota": invalid_quota},
        "hardware": {},
    }
    with pytest.raises(ValidationError, match="storage.quota"):
        validate_manifest(manifest)

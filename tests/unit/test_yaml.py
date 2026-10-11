"""
Unit and property-style generative tests for kurogane.yaml parser.
Verifies parsing of JSON, YAML, nested dictionaries, lists, booleans, and comments.
"""

import json
import os
import random
import string
import sys
import pytest

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../../src")))

from kurogane.yaml import loads


def test_loads_json_fallback():
    data = {"app": "foot", "count": 42, "active": True}
    raw_json = json.dumps(data)
    parsed = loads(raw_json)
    assert parsed == data


def test_loads_simple_yaml_mapping():
    raw_yaml = """
appliance:
  name: test-app
  version: "1.0.0"
runtime:
  engine: arch
  packages:
    - foot
    - seatd
"""
    parsed = loads(raw_yaml)
    assert isinstance(parsed, dict)
    assert parsed["appliance"]["name"] == "test-app"
    assert parsed["appliance"]["version"] == "1.0.0"
    assert parsed["runtime"]["engine"] == "arch"
    assert parsed["runtime"]["packages"] == ["foot", "seatd"]


def test_loads_yaml_with_inline_comments():
    raw_yaml = """
hardware:
  memory: 1024M  # 1GB RAM quota
  cores: 4       # multi-core allocation
  audio: true    # enable ALSA dmix
"""
    parsed = loads(raw_yaml)
    assert parsed["hardware"]["memory"] == "1024M"
    assert parsed["hardware"]["cores"] == 4
    assert parsed["hardware"]["audio"] is True


def test_generative_roundtrip_fuzzing():
    """Fuzz random nested structures and ensure loads parses without crash."""
    for _ in range(50):
        # Generate random dictionary
        depth = random.randint(1, 4)
        data = {}
        for i in range(5):
            key = f"key_{i}"
            val_type = random.choice(["int", "str", "bool", "list"])
            if val_type == "int":
                data[key] = random.randint(0, 100000)
            elif val_type == "str":
                data[key] = "".join(random.choices(string.ascii_lowercase, k=8))
            elif val_type == "bool":
                data[key] = random.choice([True, False])
            elif val_type == "list":
                data[key] = [random.randint(0, 100) for _ in range(3)]

        json_repr = json.dumps(data)
        parsed = loads(json_repr)
        assert parsed == data, f"Mismatch on {data}"

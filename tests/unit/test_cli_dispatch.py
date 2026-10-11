"""
Unit tests for kurogane.cli dispatching and subcommands.
Directly exercises CLI entrypoint parser and handlers.
"""

import os
import sys
import pytest

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../../src")))

from kurogane.cli import main


def test_cli_help_exits_zero(capsys):
    with pytest.raises(SystemExit) as excinfo:
        main(["--help"])
    assert excinfo.value.code == 0
    captured = capsys.readouterr()
    assert "usage:" in captured.out.lower() or "cartilage" in captured.out.lower() or "kurogane" in captured.out.lower()


def test_cli_validate_success(capsys):
    ret = main(["validate", "recipes/terminal-foot.yaml"])
    assert ret == 0
    captured = capsys.readouterr()
    assert "[PASS]" in captured.out


def test_cli_validate_failure(capsys):
    # Non-existent recipe
    ret = main(["validate", "recipes/non_existent_recipe.yaml"])
    assert ret == 1
    captured = capsys.readouterr()
    assert "[FAIL]" in captured.err or "[FAIL]" in captured.out

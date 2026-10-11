import os
import tempfile
import pytest
from kurogane.builder import compute_verity_merkle_tree


def test_verity_merkle_tree_basic():
    with tempfile.TemporaryDirectory() as tmpdir:
        test_file = os.path.join(tmpdir, "test_cartridge.img")
        # 16KB test image (4 blocks of 4096)
        with open(test_file, "wb") as f:
            f.write(b"A" * 4096 + b"B" * 4096 + b"C" * 4096 + b"D" * 4096)

        meta = compute_verity_merkle_tree(test_file, block_size=4096)
        assert meta["data_blocks"] == 4
        assert len(meta["root_hash"]) == 64
        assert os.path.isfile(test_file + ".verity.json")


def test_verity_bit_flip_tamper_detection():
    with tempfile.TemporaryDirectory() as tmpdir:
        test_file = os.path.join(tmpdir, "cartridge.img")
        payload = b"Z" * 8192
        with open(test_file, "wb") as f:
            f.write(payload)

        meta_orig = compute_verity_merkle_tree(test_file, block_size=4096)

        # Tamper 1 single byte in second block
        tampered_payload = b"Z" * 4096 + b"Y" + b"Z" * (4096 - 1)
        with open(test_file, "wb") as f:
            f.write(tampered_payload)

        meta_tampered = compute_verity_merkle_tree(test_file, block_size=4096)

        # Invariant: Tampered image must NEVER produce the same root hash
        assert meta_orig["root_hash"] != meta_tampered["root_hash"]

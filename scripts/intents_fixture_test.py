"""Offline identity and safe-download regressions for the Intents fixture."""
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("identity", Path(__file__).with_name("verify-intents-fixture.py"))
identity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(identity)


def valid_replacement():
    code = bytearray(b"\0asm\x01\0\0\0\0")
    remaining = identity.EXPECTED_SIZE - 12
    while True:
        byte = remaining & 127
        remaining >>= 7
        code.append(byte | (128 if remaining else 0))
        if not remaining:
            break
    code.extend(bytes(identity.EXPECTED_SIZE - len(code)))
    return code


def base58(raw):
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    number = int.from_bytes(raw, "big")
    result = ""
    while number:
        number, remainder = divmod(number, 58)
        result = alphabet[remainder] + result
    return "1" * (len(raw) - len(raw.lstrip(b"\0"))) + result


class FixtureIdentity(unittest.TestCase):
    def test_exact_reviewed_fixture_is_accepted(self):
        identity.verify(identity.FIXTURE)

    def test_valid_same_size_substitution_is_refused_by_the_cli(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "replacement.wasm"
            path.write_bytes(valid_replacement())
            result = subprocess.run([sys.executable, "-B", str(SPEC.origin), str(path)],
                                    capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 1)
            self.assertIn("SHA256 does not match", result.stderr)

    def test_different_size_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "replacement.wasm"
            path.write_bytes(b"\0asm\x01\0\0\0")
            with self.assertRaisesRegex(ValueError, "size does not match"):
                identity.verify(path)

    def test_default_fetch_preserves_reviewed_fixture_and_uses_comparison_file(self):
        original = identity.FIXTURE.read_bytes()
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            replacement = valid_replacement()
            payload = folder / "rpc.json"
            payload.write_text(json.dumps({"result": {
                "code_base64": base64.b64encode(replacement).decode(),
                "hash": base58(hashlib.sha256(replacement).digest()), "block_height": 1,
            }}))
            fake_curl = folder / "curl"
            fake_curl.write_text(f"#!{sys.executable}\nimport os,sys\nfrom pathlib import Path\n"
                                 "Path(sys.argv[sys.argv.index('-o')+1]).write_bytes(Path(os.environ['FIXTURE_RPC']).read_bytes())\n")
            fake_curl.chmod(0o755)
            env = {**os.environ, "PATH": str(folder) + os.pathsep + os.environ["PATH"],
                   "TMPDIR": str(folder), "FIXTURE_RPC": str(payload)}
            result = subprocess.run(["bash", str(Path(__file__).with_name("fetch-intents-fixture.sh"))],
                                    env=env, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            comparison = list(folder.glob("mandate-intents-comparison.*"))
            self.assertEqual(len(comparison), 1)
            self.assertEqual(comparison[0].read_bytes(), replacement)
            self.assertEqual(identity.FIXTURE.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()

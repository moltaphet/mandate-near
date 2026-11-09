"""Offline safety tests for the public deployment inspector."""
import base64
import importlib.util
import hashlib
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("deployment", Path(__file__).with_name("inspect-deployment.py"))
deployment = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(deployment)
DEFAULT_BINDING = object()


def journal_descriptor():
    return {"protocol_version": "durable_withdrawal_journal_v1", "ledger_domain": "ab" * 32,
            "executor_id": "intents.near", "request_hash_scheme": "sha256-borsh-v1"}


class FakeRpc:
    def __init__(self, mismatched=False, bad_code=False, bad_anchor=False, existing_sink=False,
                 protocol="unique_voucher_v1", binding=DEFAULT_BINDING):
        self.calls = []
        self.mismatched = mismatched
        self.bad_code = bad_code
        self.bad_anchor = bad_anchor
        self.existing_sink = existing_sink
        self.protocol = protocol
        self.binding = journal_descriptor() if binding is DEFAULT_BINDING else binding

    def status(self):
        return {"chain_id": "testnet"}

    def query(self, params):
        self.calls.append(params)
        kind = params["request_type"]
        if kind == "view_account" and params["account_id"].startswith(("s.", "c.")) and not self.existing_sink:
            raise deployment.MissingAccount("fixture sink absent")
        block = "4" * 44 if self.mismatched and "block_id" in params else "3" * 44
        result = {"block_hash": block, "block_height": 123}
        code_hash = deployment.hash_base58(hashlib.sha256(b"wasm fixture").digest())
        if kind == "view_account":
            result["code_hash"] = "invalid" if self.bad_anchor else code_hash
        if kind == "view_code":
            result.update(code_base64=base64.b64encode(b"wasm fixture").decode(),
                          hash="invalid" if self.bad_code else code_hash)
        elif kind == "view_access_key_list":
            result["keys"] = [
                {"public_key": "NEVER_EMIT_THIS", "access_key": {"permission": "FullAccess"}},
                {"public_key": "NEVER_EMIT_THIS_EITHER", "access_key": {"permission": {
                    "FunctionCall": {"receiver_id": "vault.near", "allowance": "NEVER_EMIT_ALLOWANCE"}}}},
            ]
        elif kind == "call_function":
            values = {"get_owner": "owner.near", "get_externals": ["signer.near", "intents.near", "oracle.near"],
                      "is_paused": True, "is_key_registered": False, "get_oracle_kind": "pyth",
                      "get_withdrawal_protocol": self.protocol, "public_keys_of": [],
                      "get_withdrawal_journal_binding": self.binding}
            result["result"] = list(json.dumps(values[params["method_name"]]).encode())
        return result


class InspectionTests(unittest.TestCase):
    def test_permissions_emit_only_counts_and_booleans(self):
        report = deployment.inspect(FakeRpc(), "vault.near")
        text = json.dumps(report)
        self.assertNotIn("NEVER_EMIT", text)
        self.assertNotIn("allowance", text)
        self.assertEqual(report["permissions"], {
            "total": 2, "full_access": 1, "function_call": 1, "unknown": 0, "no_access_keys": False})
        self.assertIn("no_bypass_access_keys", report["blocking_checks"])
        self.assertEqual(report["live_readiness"], "UNVERIFIED")

    def test_every_followup_is_pinned_to_the_final_anchor(self):
        rpc = FakeRpc()
        deployment.inspect(rpc, "vault.near")
        self.assertEqual(rpc.calls[0]["finality"], "final")
        self.assertTrue(all(c.get("block_id") == "3" * 44 for c in rpc.calls[1:]))
        self.assertTrue(all("finality" not in c for c in rpc.calls[1:]))

    def test_a_different_block_cannot_be_reported_as_verified(self):
        with self.assertRaisesRegex(RuntimeError, "different block"):
            deployment.inspect(FakeRpc(mismatched=True), "vault.near")

    def test_expected_code_mismatch_blocks(self):
        report = deployment.inspect(FakeRpc(), "vault.near", "0" * 64)
        self.assertFalse(report["checks"]["expected_code_matches"])
        self.assertIn("expected_code_matches", report["blocking_checks"])

    def test_code_bytes_and_account_hash_must_agree(self):
        with self.assertRaisesRegex(RuntimeError, "returned WASM"):
            deployment.inspect(FakeRpc(bad_code=True), "vault.near")
        with self.assertRaisesRegex(RuntimeError, "pinned WASM"):
            deployment.inspect(FakeRpc(bad_anchor=True), "vault.near")
        self.assertEqual(deployment.hash_base58(bytes(32)), "1" * 32)

    def test_block_metadata_rejects_malformed_hashes_and_heights(self):
        for value in ({}, "bad", "2" * 32, "0" * 44, "3" * 45):
            with self.assertRaisesRegex(RuntimeError, "block hash"):
                deployment.block_metadata({"block_hash": value, "block_height": 123})
        for value in (True, "123", -1, 2 ** 64, 1.5):
            with self.assertRaisesRegex(RuntimeError, "block height"):
                deployment.block_metadata({"block_hash": "3" * 44, "block_height": value})
        class DifferentHeight(FakeRpc):
            def query(self, params):
                result = super().query(params)
                if "block_id" in params:
                    result["block_height"] += 1
                return result
        with self.assertRaisesRegex(RuntimeError, "different block"):
            deployment.inspect(DifferentHeight(), "vault.near")

    def test_redirects_are_refused_before_another_endpoint_is_contacted(self):
        with self.assertRaisesRegex(RuntimeError, "redirects are refused"):
            deployment.NoRedirect().redirect_request(None, None, 302, "Found", {}, "http://example.com")

    def test_observed_chain_and_expected_network_are_separate(self):
        report = deployment.inspect(FakeRpc(), "vault.near", expected_chain="mainnet")
        self.assertEqual(report["chain_id"], "testnet")
        self.assertIn("expected_chain_matches", report["blocking_checks"])

    def test_receipt_sinks_must_be_absent_even_without_intents_registrations(self):
        report = deployment.inspect(FakeRpc(existing_sink=True), "vault.near")
        self.assertFalse(report["checks"]["receipt_sinks_have_no_account_or_registered_identity"])
        self.assertIn("receipt_sinks_have_no_account_or_registered_identity", report["blocking_checks"])
        self.assertTrue(all(item["registered_intents_identity_count"] == 0 for item in report["receipt_sinks"]))
        report = deployment.inspect(FakeRpc(), "vault.near")
        self.assertTrue(report["checks"]["receipt_sinks_have_no_account_or_registered_identity"])
        self.assertEqual(report["historical_issuer_receipt_quiescence"], "UNVERIFIED")

    def test_journal_binding_is_conditional_and_legacy_inspection_is_unchanged(self):
        legacy = FakeRpc()
        old = deployment.inspect(legacy, "vault.near")
        self.assertNotIn("withdrawal_journal_binding", old)
        self.assertFalse(any(call.get("method_name") == "get_withdrawal_journal_binding" for call in legacy.calls))
        self.assertEqual(len(old["receipt_sinks"]), 2)
        rpc = FakeRpc(protocol="durable_withdrawal_journal_v1", existing_sink=True)
        report = deployment.inspect(rpc, "vault.near")
        self.assertTrue(report["checks"]["control_views_valid"])
        self.assertEqual(report["withdrawal_journal_binding"], journal_descriptor())
        self.assertEqual(report["withdrawal_journal_binding_observation"], "PUBLIC_SELF_REPORT_UNVERIFIED")
        self.assertEqual(report["receipt_sinks"], "NOT_APPLICABLE")
        self.assertIsNone(report["checks"]["receipt_sinks_have_no_account_or_registered_identity"])
        self.assertEqual(report["live_readiness"], "UNVERIFIED")
        self.assertIn("authoritative journal terminal delivery or completed refund", report["unverified_obligations"])
        methods = [call.get("method_name") for call in rpc.calls]
        self.assertGreater(methods.index("get_withdrawal_journal_binding"), methods.index("get_withdrawal_protocol"))
        self.assertNotIn("public_keys_of", methods)
        self.assertFalse(any(call["account_id"].startswith(("s.", "c.")) for call in rpc.calls))
        self.assertTrue(all(call.get("block_id") == "3" * 44 for call in rpc.calls[1:]))

    def test_journal_binding_rejects_malformed_or_mismatched_descriptors(self):
        malformed = [None, {}, {**journal_descriptor(), "extra": True}]
        for field, value in (("protocol_version", "unique_voucher_v1"),
                             ("protocol_version", "durable_withdrawal_journal_v2"),
                             ("request_hash_scheme", "sha256-json-v1"),
                             ("executor_id", "other.near"), ("executor_id", "INVALID.near"),
                             ("ledger_domain", "00" * 32), ("ledger_domain", "AB" * 32),
                             ("ledger_domain", "ab" * 31), ("ledger_domain", "ab" * 32 + "\n"),
                             ("ledger_domain", 1)):
            malformed.append({**journal_descriptor(), field: value})
        for binding in malformed:
            with self.subTest(binding=binding):
                report = deployment.inspect(FakeRpc(protocol="durable_withdrawal_journal_v1", binding=binding), "vault.near")
                self.assertFalse(report["checks"]["control_views_valid"])
                self.assertIn("control_views_valid", report["blocking_checks"])
                self.assertNotIn("withdrawal_journal_binding", report)
                self.assertEqual(report["live_readiness"], "UNVERIFIED")

    def test_journal_binding_must_share_the_final_anchor_and_unknown_modes_are_refused(self):
        class ChangedBindingBlock(FakeRpc):
            def query(self, params):
                result = super().query(params)
                if params.get("method_name") == "get_withdrawal_journal_binding":
                    result["block_height"] += 1
                return result
        report = deployment.inspect(ChangedBindingBlock(protocol="durable_withdrawal_journal_v1"), "vault.near")
        self.assertFalse(report["checks"]["control_views_valid"])
        unknown = FakeRpc(protocol="durable_withdrawal_journal_v2")
        report = deployment.inspect(unknown, "vault.near")
        self.assertFalse(report["checks"]["control_views_valid"])
        self.assertFalse(any(call.get("method_name") == "get_withdrawal_journal_binding" for call in unknown.calls))

    def test_missing_account_errors_require_exact_account_and_block_binding(self):
        class Response:
            def __init__(self, payload):
                self.payload = json.dumps(payload).encode()
            def __enter__(self):
                return self
            def __exit__(self, *_args):
                return False
            def read(self, _limit):
                return self.payload
        class Opener:
            def __init__(self, account, block):
                self.account, self.block = account, block
            def open(self, *_args, **_kwargs):
                return Response({"jsonrpc": "2.0", "id": 1, "error": {"data": {"cause": {
                    "name": "UNKNOWN_ACCOUNT", "info": {"account_id": self.account, "block_hash": self.block}}}}})
        for account, block, error in (("s.vault.near", "3" * 44, deployment.MissingAccount),
                                      ("other.near", "3" * 44, RuntimeError),
                                      ("s.vault.near", "4" * 44, RuntimeError)):
            rpc = deployment.Rpc("http://127.0.0.1:3030")
            rpc.opener = Opener(account, block)
            with self.assertRaises(error) as caught:
                rpc.query({"request_type": "view_account", "account_id": "s.vault.near", "block_id": "3" * 44})
            self.assertEqual(type(caught.exception), error)

    def test_rpc_urls_do_not_accept_credentials_or_remote_plain_http(self):
        for url in ("https://user:password@example.com", "https://example.com?auth=value",
                    "http://example.com", "file:///tmp/data", "https://example.com#fragment"):
            with self.assertRaises(ValueError):
                deployment.rpc_url(url)
        self.assertEqual(deployment.rpc_url("http://127.0.0.1:3030"), "http://127.0.0.1:3030")

    def test_account_grammar_and_empty_permission_response(self):
        for value in ("a..b", "aa:bb", "a" * 65, "A.near"):
            with self.assertRaises(ValueError):
                deployment.account_id(value)
        self.assertTrue(deployment.permission_summary([])["no_access_keys"])
        with self.assertRaises(ValueError):
            deployment.permission_summary([{"access_key": None}])


if __name__ == "__main__":
    unittest.main()


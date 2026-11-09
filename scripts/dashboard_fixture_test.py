"""Offline request-handler checks for public preview routes."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("preview", Path(__file__).with_name("dashboard-fixture.py"))
preview = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(preview)


class Request:
    def __init__(self, path):
        self.path = path
        self.response = None

    def reply(self, payload, status=200, content_type="application/json"):
        self.response = (status, payload, content_type)


class PreviewRoutes(unittest.TestCase):
    def get(self, path):
        request = Request(path)
        preview.Handler.do_GET(request)
        return request.response

    def test_shared_vault_query_serves_the_site(self):
        root = self.get("/")
        shared = self.get("/?network=testnet&vault=vault.testnet&code=" + "5" * 44)
        self.assertEqual(root[0], 200)
        self.assertEqual(shared, root)

    def test_known_assets_allow_queries(self):
        self.assertEqual(self.get("/site.mjs?cache=1")[0], 200)
        self.assertEqual(self.get("/dashboard/?network=testnet")[0], 200)

    def test_queries_never_expand_the_file_allowlist(self):
        for path in ("/.env?network=testnet", "/../README.md?x=1", "/%2e%2e/.env?x=1",
                     "/missing?path=/site.mjs", "/site.mjs/missing?x=1"):
            with self.subTest(path=path):
                self.assertEqual(self.get(path)[0], 404)


if __name__ == "__main__":
    unittest.main()

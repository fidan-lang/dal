import importlib.util
from pathlib import Path
import sys
sys.dont_write_bytecode = True
import unittest
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("staging", Path(__file__).with_name("staging-smoke.py"))
staging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(staging)

class StagingTests(unittest.TestCase):
    def test_execution_requires_explicit_nonproduction_approval(self):
        for config, approved in [({}, False), ({"environment":"staging","production":False},False), ({"environment":"production","production":False},True), ({"environment":"staging","production":True},True)]:
            with self.assertRaises(ValueError): staging.validate(config, approved)
        staging.validate({"environment":"staging","production":False},True)

    def test_known_live_resources_are_never_accepted(self):
        for extra in [{"api":"https://api.dal.fidan.dev"},{"api":"https://API.DAL.FIDAN.DEV."},{"api":"https://api.dal.fidan.dev:443"},{"bucket":"FIDAN-RELEASES"},{"bucket":"dal-packages-prod"},{"bucket":"fidan-releases"}]:
            with self.assertRaises(ValueError): staging.validate({"environment":"staging","production":False,**extra},True)

    def test_auth_checks_real_status_contract_without_leaking_tokens(self):
        with patch.dict(staging.os.environ,{"DAL_STAGING_SESSION_TOKEN":"fixture-session","DAL_STAGING_SCOPED_TOKEN":"fixture-scoped"}):
            with patch.object(staging,"request",side_effect=[(200,b"",{}),(403,b"",{})]) as request:
                staging.auth({"api":"https://staging.example.test"})
                self.assertEqual(request.call_count,2)
            with patch.object(staging,"request",side_effect=[(200,b"",{}),(200,b"",{})]):
                with self.assertRaises(RuntimeError): staging.auth({"api":"https://staging.example.test"})

    def test_restore_refuses_nonisolated_target_before_any_subprocess(self):
        with patch.dict(staging.os.environ,{"DAL_STAGING_DATABASE_URL":"postgres://fixture@staging/db","DAL_STAGING_RESTORE_URL":"postgres://fixture@staging/db"}):
            with patch.object(staging,"command") as command:
                with self.assertRaises(ValueError): staging.database({})
                command.assert_not_called()

    def test_redirect_does_not_forward_authorization(self):
        seen = []
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                seen.append(self.path)
                if self.path == "/redirect":
                    self.send_response(302)
                    self.send_header("Location", "/destination")
                    self.end_headers()
                else:
                    self.send_response(200)
                    self.end_headers()
            def log_message(self, *args):
                pass
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            status, _, _ = staging.request(f"http://127.0.0.1:{server.server_port}/redirect", token="fixture-only")
            self.assertEqual(status, 302)
            self.assertEqual(seen, ["/redirect"])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_queue_requires_isolated_staging_name_before_send(self):
        config = {"aws_profile": "staging", "region": "eu-central-1", "queue_isolation": "dedicated_no_worker", "expected_aws_account": "000000000000", "queue_url": "https://sqs.eu-central-1.amazonaws.com/000000000000/production.fifo"}
        with patch.object(staging, "command", return_value='{"Account":"000000000000"}') as command:
            with self.assertRaises(ValueError):
                staging.queue(config)
            self.assertEqual(command.call_count, 1)  # STS only, no queue write.

if __name__ == "__main__": unittest.main()

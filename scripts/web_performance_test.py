import functools
import http.client
import http.server
import io
import json
import pathlib
import runpy
import tempfile
import threading
import unittest

MODULE = runpy.run_path(str(pathlib.Path(__file__).with_name('web-performance')))


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.server = http.server.HTTPServer(('127.0.0.1', 0), functools.partial(
            MODULE['Handler'], directory=self.directory.name))
        self.authority = '127.0.0.1:' + str(self.server.server_port)
        self.server.authority = self.authority
        self.server.output = io.StringIO()
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()

    def tearDown(self):
        self.server.shutdown()
        self.thread.join()
        self.server.server_close()
        self.directory.cleanup()

    def post(self, body, **overrides):
        connection = http.client.HTTPConnection(self.authority, timeout=2)
        headers = {'Origin': 'http://' + self.authority, 'Host': self.authority, **overrides}
        connection.request('POST', '/__performance/record', body=body, headers=headers)
        response = connection.getresponse()
        status = response.status
        response.read()
        connection.close()
        return status

    def get(self, path, **overrides):
        connection = http.client.HTTPConnection(self.authority, timeout=2)
        headers = {'Host': self.authority, **overrides}
        connection.request('GET', path, headers=headers)
        response = connection.getresponse()
        result = response.status, response.read()
        connection.close()
        return result

    def test_records_are_persisted_and_invalid_boundaries_rejected(self):
        self.assertEqual(self.post('{"type":"window","p95Ms":20}'), 204)
        self.assertEqual(json.loads(self.server.output.getvalue())['p95Ms'], 20)
        for body in ['{}', '[]', '{"type":"window","ms":NaN}', 'x' * 16385]:
            self.assertEqual(self.post(body), 400)
        self.assertEqual(self.post('{"type":"window"}', Origin='https://other.invalid'), 403)
        self.assertEqual(self.post('{"type":"window"}', Host='other.invalid'), 403)
        self.assertEqual(len(self.server.output.getvalue().splitlines()), 1)

    def test_auto_capture_is_query_opt_in_and_modules_are_relative(self):
        pathlib.Path(self.directory.name, 'index.html').write_text(
            '<html><body><main>bundle</main></body></html>', encoding='utf8')
        self.server.auto_capture = True
        status, body = self.get('/')
        self.assertEqual(status, 200)
        self.assertNotIn(b'autostart.mjs', body)
        status, body = self.get('/?performance')
        self.assertEqual(status, 200)
        self.assertIn(b"<script type=\"module\" src='./__performance/autostart.mjs'></script>", body)
        status, body = self.get('/index.html?seed=42')
        self.assertEqual(status, 200)
        self.assertNotIn(b'autostart.mjs', body)
        status, body = self.get('/__performance/autostart.mjs')
        self.assertEqual(status, 200)
        self.assertIn(b'./probe.mjs', body)
        self.server.auto_capture = False
        self.assertEqual(self.get('/__performance/autostart.mjs')[0], 404)

    def test_proxy_allowlist_requires_exact_host_and_origin(self):
        self.server.allowed_authority = 'turfrace.localc'
        self.server.allowed_origin = 'https://turfrace.localc'
        self.server.allowed_origins = {
            'http://' + self.authority, 'https://turfrace.localc'}
        self.assertEqual(self.post('{"type":"window"}', Host='turfrace.localc',
                                   Origin='https://turfrace.localc'), 204)
        self.assertEqual(self.post('{"type":"window"}', Host='turfrace.localc',
                                   Origin='https://evil.invalid'), 403)
        self.assertEqual(self.post('{"type":"window"}', Host='evil.invalid',
                                   Origin='https://turfrace.localc'), 403)
        self.assertEqual(self.post('{"type":"window"}', Host='turfrace.localc'), 403)

    def test_traversal_is_rejected_before_static_dispatch(self):
        pathlib.Path(self.directory.name, 'index.html').write_text('secret', encoding='utf8')
        self.server.auto_capture = True
        self.assertEqual(self.get('/%2e%2e/index.html?performance')[0], 404)
        self.assertEqual(self.get('/__performance/%2e%2e/index.html')[0], 404)


if __name__ == '__main__':
    unittest.main()

import io
import json
from contextlib import redirect_stdout
from unittest.mock import patch
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest

spec = importlib.util.spec_from_file_location("lyrics_helper", Path(__file__).with_name("youtube_music.py"))
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class Client:
    def __init__(self, result):
        self.result = result
        self.calls = []

    def get_watch_playlist(self, **kwargs):
        self.calls.append(kwargs)
        return {"lyrics": "MPLYttest="}

    def get_lyrics(self, browse, timestamps):
        self.calls.append((browse, timestamps))
        if isinstance(self.result, Exception):
            raise self.result
        return self.result


class LyricsTests(unittest.TestCase):
    def test_timed_model_objects_and_exact_video_read(self):
        client = Client({"hasTimestamps": True, "source": "Source: fixture", "lyrics": [
            SimpleNamespace(text="First line", start_time=1000, end_time=2500),
            SimpleNamespace(text="Next line", start_time=4000, end_time=6000),
        ]})
        result = helper.run(client, {"action": "lyrics", "video_id": "fixture0001"})
        self.assertTrue(result["synced"])
        self.assertEqual(result["lines"][0], {"text": "First line", "start_ms": 1000, "end_ms": 2500})
        self.assertEqual(client.calls, [{"videoId": "fixture0001", "limit": 1}, ("MPLYttest=", True)])

    def test_plain_and_absent_lyrics_are_not_auth_errors(self):
        result = helper.lyrics(Client({"lyrics": "First\n\nSecond", "hasTimestamps": False}), "fixture0001")
        self.assertFalse(result["synced"])
        self.assertEqual([l["text"] for l in result["lines"]], ["First", "Second"])
        self.assertEqual(helper.lyrics(Client(None), "fixture0001")["lines"], [])
        result = helper.lyrics(Client(RuntimeError("sensitive transport details")), "fixture0001")
        self.assertEqual(result, {"type": "error", "code": "lyrics_unavailable"})
        self.assertNotIn("sensitive", repr(result))

    def test_failed_lyrics_keep_cookie_refresh_without_exposing_transport_errors(self):
        headers = {"cookie": "__Secure-3PAPISID=fresh", "x-goog-authuser": "0"}
        session = SimpleNamespace(_tide_refreshed_headers=headers, close=lambda: None)
        client = Client(RuntimeError("sensitive transport details"))
        request = io.TextIOWrapper(io.BytesIO(json.dumps({"action": "lyrics", "video_id": "fixture0001", "headers": {}}).encode()))
        output = io.StringIO()
        with patch.object(helper, "create_client", return_value=(client, session)), patch.object(helper.sys, "stdin", request), redirect_stdout(output):
            helper.main()
        result = json.loads(output.getvalue())
        self.assertEqual(result["code"], "lyrics_unavailable")
        self.assertEqual(result["refreshed_headers"], headers)
        self.assertEqual(session._tide_request_timeout, 5)
        self.assertNotIn("sensitive", output.getvalue())

    def test_lrclib_uses_only_public_metadata_and_rejects_wrong_recordings(self):
        watch = {"tracks": [{"videoId": "fixture0001", "title": "Example", "artists": [{"name": "Artist"}], "length": "3:07"}]}
        record = {"trackName": "Example", "artistName": "Artist", "duration": 187, "syncedLyrics": "[00:01.20]First line\n[00:08.00]Next line"}
        from unittest.mock import MagicMock
        response = MagicMock()
        response.__enter__.return_value.read.return_value = json.dumps(record).encode()
        with patch("urllib.request.urlopen", return_value=response) as read:
            result = helper.lrclib_lyrics(watch, "fixture0001")
        self.assertTrue(result["synced"])
        self.assertEqual(result["lines"][0]["start_ms"], 1200)
        request = read.call_args.args[0]
        self.assertTrue(request.full_url.startswith("https://lrclib.net/api/get?"))
        self.assertIsNone(request.get_header("Cookie"))
        self.assertIsNone(request.get_header("Authorization"))
        record["duration"] = 191
        response.__enter__.return_value.read.return_value = json.dumps(record).encode()
        with patch("urllib.request.urlopen", return_value=response):
            self.assertIsNone(helper.lrclib_lyrics(watch, "fixture0001"))
        self.assertIsNone(helper.lrc_lines("[offset:100]\n[00:00.00]A", 187))
        self.assertIsNone(helper.lrc_lines("[99:00.00]A", 187))

    def test_searches_for_timed_record_when_youtube_has_no_lyrics_link(self):
        from unittest.mock import MagicMock
        watch = {"tracks": [{"videoId": "fixture0001", "title": "Example",
                             "artists": [{"name": "Artist"}], "length": "3:07"}]}
        client = Client(None)
        client.get_watch_playlist = MagicMock(return_value=watch)
        exact = {"trackName": "Example", "artistName": "Artist", "duration": 187}
        timed = {**exact, "syncedLyrics": "[00:01.20]First line"}
        response = MagicMock()
        response.__enter__.return_value.read.side_effect = [
            json.dumps(exact).encode(),
            json.dumps([{**timed, "duration": 199}, timed]).encode(),
        ]
        with patch("urllib.request.urlopen", return_value=response) as read:
            result = helper.lyrics(client, "fixture0001")
        self.assertTrue(result["synced"])
        self.assertEqual(result["lines"][0]["start_ms"], 1200)
        self.assertIn("/api/search?", read.call_args.args[0].full_url)
        self.assertEqual(client.calls, [])

    def test_word_timings_use_utf8_boundaries_and_reject_yaml_aliases(self):
        document = """version: '1.0'
lines:
  - text: 'Café noir'
    start_ms: 1000
    end_ms: 2400
    words:
      - text: 'Café '
        start_ms: 1000
      - text: 'noir'
        start_ms: 1800
"""
        result = helper.word_timed_lines(document, 3)
        self.assertEqual(result[0]["words"], [
            {"start_ms": 1000, "end_byte": 6},
            {"start_ms": 1800, "end_byte": 10},
        ])
        self.assertIsNone(helper.word_timed_lines(document.replace("1800", "2600"), 3))
        self.assertIsNone(helper.word_timed_lines(document + "copy: &a [x, *a]", 3))

    def test_lrclib_rate_limit_is_returned_for_shared_cooldown(self):
        from urllib.error import HTTPError
        from email.message import Message
        watch = {"tracks": [{"videoId": "fixture0001", "title": "Example", "artists": [{"name": "Artist"}], "length": "3:07"}]}
        headers = Message()
        headers["Retry-After"] = "120"
        error = HTTPError("https://lrclib.net/api/get", 429, "Rate limited", headers, None)
        with patch("urllib.request.urlopen", side_effect=error):
            result = helper.lrclib_lyrics(watch, "fixture0001")
        self.assertEqual(result["retry_after_seconds"], 120)
        self.assertEqual(result["lines"], [])
        with patch.object(helper, "lrclib_lyrics") as lookup:
            helper.run(Client({"hasTimestamps": False, "lyrics": "Plain"}), {"action": "lyrics", "video_id": "fixture0001", "allow_external": False})
            lookup.assert_not_called()

    def test_limits_bad_times_and_invalid_ids(self):
        client = Client(None)
        self.assertEqual(helper.lyrics(client, "not a video")["lines"], [])
        self.assertEqual(client.calls, [])
        for lines in [
            [{"text": "a", "start_time": True}],
            [{"text": "a", "start_time": -1}],
            [{"text": "a", "start_time": 10, "end_time": 9}],
            [{"text": "a", "start_time": 100}, {"text": "b", "start_time": 50}],
        ]:
            self.assertEqual(helper.lyrics(Client({"hasTimestamps": True, "lyrics": lines}), "fixture0001")["lines"], [])
        result = helper.lyrics(Client({"hasTimestamps": False, "lyrics": "x" * 128001}), "fixture0001")
        self.assertEqual(result["lines"], [])


if __name__ == "__main__":
    unittest.main()

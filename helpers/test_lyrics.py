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

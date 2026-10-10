import unittest
from youtube_music import run, tracks


class FakeClient:
    def get_account_info(self):
        return {"accountName": "Fixture listener"}

    def get_library_playlists(self, limit):
        return [{"playlistId": "PLfixture", "title": "Queue", "count": 2}] * limit

    def get_playlist(self, playlist, limit):
        return {"tracks": [{"videoId": "K4DyBUG242c", "title": "Song"}] * limit}

    def get_liked_songs(self, limit):
        return self.get_playlist("LM", limit)

    def get_home(self, limit):
        return [{"title": "For you", "contents": [
            {"videoId": "K4DyBUG242c", "title": "Song"},
            {"type": "album", "browseId": "MPREfixture", "title": "Album"},
            {"playlistId": "PLfixture", "title": "Mix"},
        ] * 20}] * 4


class ProviderTests(unittest.TestCase):
    def test_library_limit_and_identity(self):
        result = run(FakeClient(), {"action": "library"})
        self.assertEqual(result["account"], "Fixture listener")
        self.assertEqual(len(result["playlists"]), 200)
        self.assertTrue(result["truncated"])

    def test_tracks_limit_and_safe_source(self):
        result = run(FakeClient(), {"action": "liked"})
        self.assertEqual(len(result["tracks"]), 2000)
        self.assertTrue(result["truncated"])
        self.assertEqual(result["tracks"][0]["source"], "https://www.youtube.com/watch?v=K4DyBUG242c")
        self.assertEqual(tracks([{"videoId": "https://other.example"}]), [])

    def test_home_types_and_limit(self):
        result = run(FakeClient(), {"action": "home"})
        self.assertEqual(len(result["cards"]), 120)
        self.assertEqual([c["kind"] for c in result["cards"][:3]], ["song", "album", "playlist"])

    def test_rejects_invalid_playlist_and_writes(self):
        for request in [{"action": "playlist", "id": "bad/id"}, {"action": "rate"}]:
            with self.assertRaises(ValueError):
                run(FakeClient(), request)


if __name__ == "__main__":
    unittest.main()

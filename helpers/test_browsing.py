"""Read-only discovery contracts; no credentials, network, or account writes."""
import copy
import json
import unittest
from types import SimpleNamespace
from unittest.mock import MagicMock

from youtube_music import MAX_LIBRARY_CARDS, MAX_TRACKS, browse_cards, run


class BrowsingTests(unittest.TestCase):
    video = "K4DyBUG242c"
    artist = "UC1234567890123456789012"
    art = "https://i.ytimg.com/vi/K4DyBUG242c/hqdefault.jpg"

    def test_library_playlist_count_distinguishes_empty_from_unknown(self):
        entries = [{"playlistId": f"PLfixture{i}", "title": "Playlist", "count": count}
                   for i, count in enumerate([None, 0, "1", "1,250", True, "unknown"])]
        client = SimpleNamespace(get_account_info=MagicMock(return_value={"accountName": "Listener"}),
                                 get_library_playlists=MagicMock(return_value=entries))
        result = run(client, {"action": "library"})
        self.assertEqual([entry["subtitle"] for entry in result["playlists"]],
                         ["", "0 tracks", "1 track", "1,250 tracks", "", ""])

    def test_search_is_typed_bounded_and_does_not_mutate_provider_items(self):
        cases = [
            ("songs", "song", {"videoId": self.video, "title": "Song", "artists": [{"id": self.artist, "name": "Band"}]}),
            ("albums", "album", {"browseId": "MPREb_fixture", "title": "Album", "artist": "Band"}),
            ("artists", "artist", {"browseId": self.artist, "artist": "Band"}),
            ("playlists", "playlist", {"browseId": "VLPLfixture", "title": "Playlist", "author": "Listener"}),
        ]
        for kind, result_type, fields in cases:
            with self.subTest(kind=kind):
                item = dict(fields, resultType=result_type, feedbackToken="private-fixture-token",
                            thumbnails=[{"url": self.art, "width": 160, "height": 160}])
                original = copy.deepcopy(item)
                client = SimpleNamespace(search=MagicMock(return_value=[item, dict(item, resultType="podcast")]))
                result = run(client, {"action": "search", "query": " Band ", "kind": kind})
                client.search.assert_called_once_with("Band", filter=kind, limit=40)
                self.assertEqual((result["type"], result["kind"]), ("search", kind))
                self.assertEqual(len(result["cards"]), 1)
                self.assertEqual(result["cards"][0]["kind"], result_type)
                self.assertEqual(item, original)
                self.assertNotIn("private-fixture-token", json.dumps(result))
                if kind == "playlists":
                    self.assertEqual(result["cards"][0]["id"], "PLfixture")
                    self.assertEqual(result["cards"][0]["subtitle"], "Listener")
                if kind == "albums":
                    self.assertEqual(result["cards"][0]["subtitle"], "Band")

    def test_search_rejects_malformed_inputs_before_any_request(self):
        client = SimpleNamespace(search=MagicMock())
        for kind, query in [("video", "Band"), ({}, "Band"), ("songs", ""), ("songs", "a\nheader"),
                            ("songs", "א" * 257), ("songs", None)]:
            self.assertEqual(run(client, {"action": "search", "query": query, "kind": kind}),
                             {"type": "error", "code": "search_unavailable"})
        client.search.assert_not_called()
        client.search.return_value = [{"resultType": "song", "videoId": self.video, "title": "Song"}] * 100
        self.assertEqual(len(run(client, {"action": "search", "query": "Band", "kind": "songs"})["cards"]), 40)

    def test_saved_albums_are_bounded_deduplicated_and_strictly_normalized(self):
        item = {"browseId": "MPREb_fixture", "title": "Album\n",
                "artists": [{"id": self.artist, "name": "Band"}],
                "thumbnails": [{"url": self.art, "width": 120, "height": 120}],
                "feedbackToken": "private-fixture-token"}
        client = SimpleNamespace(get_library_albums=MagicMock(return_value=[
            None, {"browseId": "https://evil.invalid", "title": "Bad"}, item, item,
        ]))
        result = run(client, {"action": "libraryalbums"})
        self.assertEqual((result["type"], result["section"]), ("browse", "albums"))
        client.get_library_albums.assert_called_once_with(limit=201)
        self.assertEqual(len(result["cards"]), 1)
        self.assertEqual(result["cards"][0]["title"], "Album")
        self.assertEqual(result["cards"][0]["kind"], "album")
        self.assertEqual(result["cards"][0]["artwork_url"], self.art)
        self.assertNotIn("private-fixture-token", json.dumps(result))
        client.get_library_albums.return_value = [
            dict(item, browseId=f"MPREb_album_{i}") for i in range(250)]
        result = run(client, {"action": "libraryalbums"})
        self.assertEqual(len(result["cards"]), MAX_LIBRARY_CARDS)
        self.assertTrue(result["truncated"])

    def test_library_artist_uses_canonical_id_and_actual_artist_field(self):
        client = SimpleNamespace(get_library_artists=MagicMock(return_value=[
            {"browseId": self.artist, "artist": "A band"},
            {"browseId": "UCbad", "artist": "Invalid ID"},
            {"browseId": self.artist, "artist": "Duplicate"},
        ]))
        result = run(client, {"action": "libraryartists"})
        self.assertEqual((result["type"], result["section"]), ("browse", "artists"))
        client.get_library_artists.assert_called_once_with(limit=201)
        self.assertEqual([(c["id"], c["title"], c["kind"]) for c in result["cards"]],
                         [(self.artist, "A band", "artist")])

    def test_history_keeps_provider_order_and_repeated_listens_without_writes(self):
        first = {"videoId": self.video, "title": "A song", "played": "Today",
                 "duration": "3:42", "feedbackToken": "do-not-return"}
        second = dict(first, played="Yesterday")
        client = MagicMock()
        client.get_history.return_value = [first, second]
        result = run(client, {"action": "history"})
        self.assertEqual((result["type"], result["section"]), ("browse", "history"))
        self.assertEqual([c["id"] for c in result["cards"]], [self.video, self.video])
        self.assertEqual([c["shelf"] for c in result["cards"]], ["Today", "Yesterday"])
        self.assertEqual(result["cards"][0]["duration_seconds"], 222)
        self.assertNotIn("do-not-return", json.dumps(result))
        self.assertEqual(client.mock_calls, [("get_history", (), {})])

    def test_album_tracks_inherit_album_art_and_artist_without_mutating_response(self):
        data = {"title": "The album", "artists": [{"id": self.artist, "name": "Band"}],
                "thumbnails": [{"url": self.art, "width": 120, "height": 120}],
                "tracks": [{"videoId": self.video, "title": "First"},
                           {"videoId": "00000000001", "title": "Second",
                            "artists": [{"id": self.artist, "name": "Guest"}]}]}
        original = copy.deepcopy(data)
        client = SimpleNamespace(get_album=MagicMock(return_value=data))
        result = run(client, {"action": "album", "id": "MPREb_fixture"})
        self.assertEqual(data, original)
        self.assertEqual([t["title"] for t in result["tracks"]], ["First", "Second"])
        self.assertEqual([t["artist"] for t in result["tracks"]], ["Band", "Guest"])
        self.assertEqual(result["tracks"][0]["album"], "The album")
        self.assertEqual(result["tracks"][0]["artwork_url"], self.art)
        self.assertFalse(result["truncated"])
        data["tracks"] *= MAX_TRACKS
        result = run(client, {"action": "album", "id": "MPREb_fixture"})
        self.assertEqual(len(result["tracks"]), MAX_TRACKS)
        self.assertTrue(result["truncated"])

    def test_album_rejects_non_ids_before_touching_client(self):
        client = MagicMock()
        for value in (None, True, "", "MPRE", "https://evil.invalid/MPREb_bad", "PLplaylist", "MPRE\n"):
            with self.subTest(value=value):
                self.assertEqual(run(client, {"action": "album", "id": value}),
                                 {"type": "error", "code": "album_unavailable"})
        self.assertEqual(client.mock_calls, [])

    def test_malformed_unavailable_and_private_art_are_not_exposed(self):
        items = [None, {}, {"videoId": self.video, "title": "Hidden", "isAvailable": False},
                 {"videoId": self.video, "title": "Visible", "thumbnails": [
                     {"url": "http://127.0.0.1/private.png"}]}]
        cards = browse_cards(items, "song", "Listening history")
        self.assertEqual(len(cards), 1)
        self.assertIsNone(cards[0]["artwork_url"])
        self.assertEqual(cards[0]["shelf"], "Listening history")
        self.assertEqual(browse_cards(None, "song", "History"), [])


if __name__ == "__main__":
    unittest.main()

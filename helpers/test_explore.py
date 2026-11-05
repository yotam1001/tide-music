"""Explore stays separate from personalized Home and never forwards opaque raw replies."""
import copy
import json
import unittest
from types import SimpleNamespace
from unittest.mock import MagicMock
from youtube_music import run

class ExploreTests(unittest.TestCase):
    song = {"videoId": "K4DyBUG242c", "title": "Music", "artists": [{"name": "Band"}]}

    def test_explore_retains_shelf_order_and_excludes_podcasts_and_api_tokens(self):
        data = {"new_releases": [{"browseId": "MPREb_fixture", "title": "Album"}],
                "top_songs": {"items": [dict(self.song, feedbackToken="private-fixture-token")]},
                "trending": {"items": [dict(self.song, title="Trending"), dict(self.song, podcast={"name":"Talk"})]}}
        original = copy.deepcopy(data)
        client = SimpleNamespace(get_explore=MagicMock(return_value=data), get_mood_categories=MagicMock(return_value={
            "Moods": [{"title":"Chill", "params":"public_chill"}], "Genres": [{"title":"Metal", "params":"public_metal"}]}))
        result = run(client, {"action":"explore"})
        self.assertEqual(result["type"], "explore")
        self.assertEqual([card["shelf"] for card in result["cards"]], ["New releases", "Top songs", "Trending"])
        self.assertEqual([item["group"] for item in result["categories"]], ["Moods", "Genres"])
        self.assertTrue(result["categories_available"])
        self.assertEqual(data, original)
        self.assertNotIn("private-fixture-token", json.dumps(result))
        self.assertFalse(result["truncated"])

    def test_explore_bounds_chart_and_category_results(self):
        client = SimpleNamespace(get_explore=MagicMock(return_value={"top_songs":{"items":[self.song]*1000}}),
            get_mood_categories=MagicMock(return_value={"Moods":[{"title":str(i),"params":f"public_{i}"} for i in range(140)]}))
        result=run(client,{"action":"explore"})
        self.assertEqual(len(result["cards"]),40)
        self.assertEqual(len(result["categories"]),128)
        self.assertTrue(result["truncated"])

    def test_category_failure_keeps_valid_music(self):
        client=SimpleNamespace(get_explore=MagicMock(return_value={"top_songs":{"items":[self.song]}}),
            get_mood_categories=MagicMock(side_effect=RuntimeError("private fixture data")))
        result=run(client,{"action":"explore"})
        self.assertEqual(len(result["cards"]),1)
        self.assertFalse(result["categories_available"])
        self.assertEqual(result["categories"],[])
        self.assertNotIn("private fixture data",json.dumps(result))

    def test_mood_selection_validates_parameters_and_normalizes_only_playlists(self):
        item={"playlistId":"VLPLfixture","title":"Chill music","author":{"name":"YouTube Music"},"token":"private-fixture-token"}
        client=SimpleNamespace(get_mood_playlists=MagicMock(return_value=[item]))
        for params in [None,{},"","x\nheader","x"*2049,"mood with spaces"]:
            self.assertEqual(run(client,{"action":"mood","params":params}),{"type":"error","code":"explore_unavailable"})
        client.get_mood_playlists.assert_not_called()
        result=run(client,{"action":"mood","params":"public_chill"})
        client.get_mood_playlists.assert_called_once_with("public_chill")
        self.assertEqual(result["cards"][0]["id"],"PLfixture")
        self.assertEqual(result["cards"][0]["subtitle"],"YouTube Music")
        self.assertNotIn("private-fixture-token",json.dumps(result))
        self.assertEqual(item["playlistId"],"VLPLfixture")

if __name__ == "__main__":
    unittest.main()

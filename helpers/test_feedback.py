"""Account-free checks: these tests never load a session or send requests."""
from types import SimpleNamespace
from unittest.mock import MagicMock
from unittest.mock import patch
import io
import json
import unittest

from youtube_music import feedback, playlist_context, run, main


class FeedbackTests(unittest.TestCase):
    video = "K4DyBUG242c"

    def client(self, owned=True):
        return SimpleNamespace(
            get_playlist=MagicMock(return_value={"owned": owned, "tracks": [
                {"videoId": self.video, "setVideoId": "entry-one"},
                {"videoId": self.video, "setVideoId": "entry-two"}]}),
            rate_song=MagicMock(return_value={"responseContext": {}}),
            add_playlist_items=MagicMock(return_value={"status": "STATUS_SUCCEEDED"}),
            remove_playlist_items=MagicMock(return_value="STATUS_SUCCEEDED"),
            get_song=MagicMock(return_value={"playbackTracking": {
                "videostatsPlaybackUrl": {"baseUrl": "https://s.youtube.com/api/stats/playback?fixture=private"}}}),
            add_history_item=MagicMock(return_value=SimpleNamespace(status_code=204)),
            get_watch_playlist=MagicMock(return_value={"tracks": [
                {"videoId": "00000000001", "likeStatus": "DISLIKE"},
                {"videoId": self.video, "likeStatus": "LIKE"}]}),
        )

    def request(self, kind, **values):
        return {"kind": kind, "video_id": self.video, **values}

    def test_strict_input_never_reaches_mutation(self):
        client = self.client()
        cases = [None, [], {}, self.request("unknown"),
                 self.request("rate", rating="LIKE", cookie="not-accepted"),
                 self.request("rate", rating="like"),
                 self.request("history", video_id="https://youtube.com/watch?v=" + self.video),
                 self.request("playlistadd", playlist_id="https://example.test/a"),
                 self.request("playlistremove", playlist_id="PLfixture", entry_id="../file")]
        for request in cases:
            self.assertEqual(feedback(client, request)["code"], "feedback_invalid")
        for name in vars(client):
            getattr(client, name).assert_not_called()

    def test_rating_is_explicit_and_returns_no_provider_data(self):
        for rating in ("LIKE", "DISLIKE", "INDIFFERENT"):
            client = self.client()
            result = feedback(client, self.request("rate", rating=rating))
            self.assertEqual(result["result"]["rating"], rating)
            client.rate_song.assert_called_once_with(self.video, rating)
            self.assertNotIn("responseContext", json.dumps(result))

    def test_inspection_matches_the_requested_video(self):
        result = feedback(self.client(), self.request("rating"))
        self.assertEqual(result["result"]["rating"], "LIKE")

    def test_add_verifies_ownership_and_rejects_duplicates(self):
        client = self.client()
        result = feedback(client, self.request("playlistadd", playlist_id="PLfixture"))
        self.assertEqual(result["type"], "feedback")
        client.get_playlist.assert_called_once_with("PLfixture", limit=1)
        client.add_playlist_items.assert_called_once_with("PLfixture", [self.video], duplicates=False)
        for status in ({"status": "NOT_SUCCEEDED"}, {}, None):
            client = self.client()
            client.add_playlist_items.return_value = status
            self.assertEqual(feedback(client, self.request("playlistadd", playlist_id="PLfixture"))["code"],
                             "feedback_unconfirmed")

    def test_remove_targets_one_exact_duplicate_entry(self):
        client = self.client()
        result = feedback(client, self.request("playlistremove", playlist_id="PLfixture", entry_id="entry-two"))
        self.assertEqual(result["type"], "feedback")
        client.remove_playlist_items.assert_called_once_with(
            "PLfixture", [{"videoId": self.video, "setVideoId": "entry-two"}])
        client = self.client()
        result = feedback(client, self.request("playlistremove", playlist_id="PLfixture", entry_id="entry-gone"))
        self.assertEqual(result["code"], "feedback_entry_changed")
        client.remove_playlist_items.assert_not_called()

    def test_ownership_must_be_verified_boolean_true(self):
        for owned in (False, None, "true", 1, {}):
            for kind, extra in (("playlistadd", {}), ("playlistremove", {"entry_id": "entry-one"})):
                client = self.client(owned)
                result = feedback(client, self.request(kind, playlist_id="PLfixture", **extra))
                self.assertEqual(result["code"], "feedback_readonly")
                client.add_playlist_items.assert_not_called()
                client.remove_playlist_items.assert_not_called()

    def test_failed_write_is_never_replayed_or_exposed(self):
        client = self.client()
        client.rate_song.side_effect = TimeoutError("private tracking URL and cookie fixture")
        result = feedback(client, self.request("rate", rating="LIKE"))
        self.assertEqual(result, {"type": "error", "code": "feedback_unconfirmed"})
        client.rate_song.assert_called_once()

    def test_uncertain_write_still_returns_safe_rotated_cookie_for_local_persistence(self):
        client = self.client()
        client.rate_song.side_effect = TimeoutError("do not disclose fixture response")
        session = SimpleNamespace(_tide_refreshed_headers={"cookie": "fixture only"}, close=MagicMock())
        request = {"action": "feedback", "headers": {}, "request": self.request("rate", rating="LIKE")}
        stdin = SimpleNamespace(buffer=io.BytesIO(json.dumps(request).encode()))
        stdout = io.StringIO()
        with patch("youtube_music.create_client", return_value=(client, session)), \
                patch("youtube_music.sys.stdin", stdin), patch("youtube_music.sys.stdout", stdout):
            main()
        result = json.loads(stdout.getvalue())
        self.assertEqual(result["code"], "feedback_unconfirmed")
        self.assertEqual(result["refreshed_headers"], session._tide_refreshed_headers)
        self.assertNotIn("do not disclose", stdout.getvalue())
        client.rate_song.assert_called_once()
        session.close.assert_called_once()

    def test_history_requires_same_client_and_success_ack(self):
        client = self.client()
        result = feedback(client, self.request("history"))
        self.assertEqual(result["type"], "feedback")
        client.get_song.assert_called_once_with(self.video)
        client.add_history_item.assert_called_once_with(client.get_song.return_value)
        self.assertNotIn("private", json.dumps(result))
        client.add_history_item.return_value.status_code = 200
        self.assertEqual(feedback(client, self.request("history"))["code"], "feedback_unconfirmed")

    def test_history_never_follows_untrusted_tracking_destinations(self):
        for url in ("http://s.youtube.com/api/stats/playback", "https://youtube.com.evil.test/api/stats/playback",
                    "https://user@s.youtube.com/api/stats/playback", "https://s.youtube.com:444/api/stats/playback",
                    "https://s.youtube.com/other", "https://s.youtube.com/api/stats/playback#fragment",
                    "https://127.0.0.1/api/stats/playback", "https://s.youtube.com/api/stats/playback\n"):
            client = self.client()
            client.get_song.return_value["playbackTracking"]["videostatsPlaybackUrl"]["baseUrl"] = url
            self.assertEqual(feedback(client, self.request("history"))["code"], "feedback_unconfirmed")
            client.add_history_item.assert_not_called()

    def test_playlist_context_keeps_entry_alignment_and_duplicates(self):
        context = playlist_context("PLfixture", {"owned": True, "tracks": [
            {"videoId": self.video, "setVideoId": "one"},
            {"videoId": self.video, "setVideoId": "skip", "isAvailable": False},
            {"videoId": "bad", "setVideoId": "skip"},
            {"videoId": self.video, "setVideoId": "two"},
            {"videoId": self.video, "setVideoId": "invalid/entry"},
        ]})
        self.assertTrue(context["editable"])
        self.assertEqual(context["entries"], ["one", "two", None])

    def test_browsing_does_not_record_history_or_rate_songs(self):
        client = self.client()
        client.get_account_info = MagicMock(return_value={"accountName": "Fixture"})
        client.get_library_playlists = MagicMock(return_value=[])
        run(client, {"action": "library"})
        run(client, {"action": "radio", "video_id": self.video})
        client.add_history_item.assert_not_called()
        client.rate_song.assert_not_called()


if __name__ == "__main__":
    unittest.main()

import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch
from youtube_music import (LOGIN_SECONDS, MUSIC_URL, authenticated_request, connect, main,
                          music_cookies, music_url, remove_login_profile, run,
                          session_headers, tracks, wait_for_login)


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


class LoginTests(unittest.TestCase):
    COOKIE = {"name": "__Secure-3PAPISID", "value": "fixture",
              "domain": ".youtube.com", "path": "/"}

    def request(self, url=MUSIC_URL + "/youtubei/v1/browse", **changes):
        values = {"url": url, "method": "POST", "headers": {
            "authorization": "SAPISIDHASH fixture", "x-goog-authuser": "0"}}
        values.update(changes)
        request = SimpleNamespace(**values)
        request.all_headers = lambda: {**request.headers, "Cookie": "__Secure-3PAPISID=fixture"}
        return request

    def test_host_and_authentication_scope(self):
        self.assertTrue(authenticated_request(self.request()))
        for url in ["https://music.youtube.com.evil.test/youtubei/v1/browse",
                    "https://accounts.google.com/youtubei/v1/browse",
                    "http://music.youtube.com/youtubei/v1/browse",
                    MUSIC_URL + ":444/youtubei/v1/browse", MUSIC_URL + "/watch",
                    "https://user@music.youtube.com/youtubei/v1/browse"]:
            self.assertFalse(authenticated_request(self.request(url)))
        self.assertFalse(music_url("https://music.youtube.com:invalid"))
        self.assertFalse(authenticated_request(self.request(method="GET")))
        self.assertFalse(authenticated_request(self.request(headers={"x-goog-authuser": "0"})))
        self.assertFalse(authenticated_request(self.request(headers={"authorization": "Bearer fixture"})))

    def test_normalizes_only_selected_headers(self):
        headers = session_headers({"Cookie": "__Secure-3PAPISID=fixture; SID=fixture",
                                   "X-Goog-AuthUser": "2", "X-Goog-PageId": "123",
                                   "User-Agent": "Fixture Browser", "Origin": "https://wrong.test",
                                   "Authorization": "SAPISIDHASH fixture",
                                   "X-Other-Secret": "fixture"})
        self.assertEqual(headers["x-goog-authuser"], "2")
        self.assertEqual(headers["x-goog-pageid"], "123")
        self.assertEqual(headers["origin"], MUSIC_URL)
        self.assertNotIn("authorization", headers)
        self.assertNotIn("x-other-secret", headers)
        for invalid in [{"cookie": "SID=fixture", "x-goog-authuser": "0"},
                        {"cookie": "__Secure-3PAPISID=fixture", "x-goog-authuser": "0\r\nx:bad"},
                        {"cookie": "__Secure-3PAPISID=fixture", "x-goog-authuser": "0",
                         "x-goog-pageid": "invalid"}]:
            self.assertIsNone(session_headers(invalid))

    def test_cookie_fallback_is_scoped_and_bounded(self):
        cookies = [self.COOKIE, {**self.COOKIE, "name": "GoogleSecret", "domain": ".google.com"},
                   {**self.COOKIE, "name": "LookalikeSecret", "domain": ".youtube.com.evil.test"},
                   {**self.COOKIE, "name": "BadValue", "value": "bad;value"},
                   {**self.COOKIE, "name": "WrongPath", "path": "/watch"}]
        result = session_headers({"x-goog-authuser": "0"}, cookies)
        self.assertEqual(result["cookie"], "__Secure-3PAPISID=fixture")
        self.assertIsNone(session_headers({"x-goog-authuser": "0"}, [cookies[1]]))
        self.assertEqual(music_cookies([{**self.COOKIE, "value": "x" * 4097}]), "")
        self.assertEqual(music_cookies([{**self.COOKIE, "name": "C" + str(i), "value": "x" * 4096}
                                       for i in range(20)]), "")

    def context(self, page_url=MUSIC_URL, events=(), step=1):
        clock = [0]
        handlers = {}
        events = iter(events)
        cookie_urls = []
        page = SimpleNamespace(url=page_url, is_closed=lambda: False)

        def wait(_milliseconds):
            clock[0] += step
            response = next(events, None)
            if response is not None:
                handlers["response"](response)

        def cookies(url):
            cookie_urls.append(url)
            return [self.COOKIE]

        page.wait_for_timeout = wait
        context = SimpleNamespace(pages=[page], on=lambda event, callback: handlers.update({event: callback}),
                                  cookies=cookies)
        return context, clock, cookie_urls

    def test_capture_requires_music_return_and_successful_browse(self):
        response = SimpleNamespace(status=200, request=self.request())
        context, clock, cookie_urls = self.context(events=[response])
        with patch("youtube_music.time.monotonic", side_effect=lambda: clock[0]):
            result = wait_for_login(context, timeout=5)
        self.assertEqual(result["type"], "session")
        self.assertEqual(cookie_urls, [MUSIC_URL])
        for page_url, status in [("https://accounts.google.com/signin", 200), (MUSIC_URL, 401)]:
            context, clock, cookie_urls = self.context(page_url, [SimpleNamespace(status=status, request=self.request())])
            with patch("youtube_music.time.monotonic", side_effect=lambda: clock[0]):
                self.assertEqual(wait_for_login(context, timeout=3)["code"], "login_timeout")
            self.assertEqual(cookie_urls, [])

    def test_unrelated_requests_never_read_full_headers(self):
        request = self.request("https://accounts.google.com/youtubei/v1/browse")
        request.all_headers = lambda: self.fail("Must not inspect Google request cookies")
        context, clock, cookie_urls = self.context(events=[SimpleNamespace(status=200, request=request)])
        with patch("youtube_music.time.monotonic", side_effect=lambda: clock[0]):
            self.assertEqual(wait_for_login(context, timeout=2)["code"], "login_timeout")
        self.assertEqual(cookie_urls, [])

    def test_closed_rejected_and_eight_minute_limit(self):
        context, clock, _ = self.context(step=100)
        with patch("youtube_music.time.monotonic", side_effect=lambda: clock[0]):
            self.assertEqual(wait_for_login(context, timeout=99999)["code"], "login_timeout")
        self.assertLessEqual(clock[0], LOGIN_SECONDS + 100)
        context.pages = []
        self.assertEqual(wait_for_login(context)["code"], "login_closed")
        context, _, _ = self.context("https://accounts.google.com/signin/rejected")
        self.assertEqual(wait_for_login(context)["code"], "login_blocked")

    def test_removal_only_deletes_owned_child_profile(self):
        with tempfile.TemporaryDirectory() as private, tempfile.TemporaryDirectory() as unrelated:
            profile = Path(tempfile.mkdtemp(prefix="edge-login-", dir=private))
            (profile / "fixture").write_text("fixture")
            self.assertFalse(remove_login_profile(Path(unrelated), Path(private)))
            self.assertTrue(Path(unrelated).exists())
            self.assertTrue(remove_login_profile(profile, Path(private)))
            self.assertTrue(Path(private).exists())
            self.assertFalse(profile.exists())

    def test_connect_dispatches_before_provider_imports(self):
        stdin = SimpleNamespace(buffer=io.BytesIO(b'{"action":"connect","directory":"fixture"}'))
        stdout = io.StringIO()
        with patch("youtube_music.sys.stdin", stdin), patch("youtube_music.sys.stdout", stdout), \
                patch("youtube_music.connect", return_value={"type": "error", "code": "login_closed"}) as connect:
            main()
        connect.assert_called_once_with("fixture")
        self.assertEqual(json.loads(stdout.getvalue()), {"type": "error", "code": "login_closed"})

    def test_connect_uses_local_sandboxed_edge_and_cleans_profile(self):
        playwright = MagicMock()
        manager = MagicMock()
        manager.__enter__.return_value = playwright
        context = playwright.chromium.launch_persistent_context.return_value
        context.pages = [MagicMock()]
        api = SimpleNamespace(sync_playwright=lambda: manager)
        with tempfile.TemporaryDirectory() as private, \
                patch.dict("sys.modules", {"playwright.sync_api": api}), \
                patch.dict("youtube_music.os.environ", {"DEBUG": "pw:protocol", "PWDEBUG": "1",
                                                       "SELENIUM_REMOTE_URL": "https://fixture.invalid"}), \
                patch("youtube_music.wait_for_login", return_value={"type": "error", "code": "login_closed"}):
            self.assertEqual(connect(private)["code"], "login_closed")
            args, options = playwright.chromium.launch_persistent_context.call_args
            self.assertEqual(Path(args[0]).parent, Path(private).resolve())
            self.assertEqual(options["channel"], "msedge")
            self.assertFalse(options["headless"])
            self.assertTrue(options["chromium_sandbox"])
            context.close.assert_called_once()
            self.assertEqual(list(Path(private).iterdir()), [])


if __name__ == "__main__":
    unittest.main()

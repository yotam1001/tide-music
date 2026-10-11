import io
import os
from contextlib import closing
import json
import sqlite3
import time
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch
from youtube_music import (LOGIN_SECONDS, MAX_RADIO_TRACKS, MUSIC_URL, artwork_url, connect, connect_existing, create_client, main,
                          music_cookies, read_cookie_database, CookieReadError, read_music_cookies, remove_login_profile, run,
                          selected_zen_database, session_headers, switch_account, tracks,
                          validate_session, wait_for_login, zen_executable, request_failure)


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
    def test_anonymous_account_response_is_session_failure_without_private_data(self):
        def get_account_info(flag):
            response = {"responseContext": {"serviceTrackingParams": [
                {"params": [{"key": "logged_in", "value": flag}]}]},
                "private": "never disclose fixture"}
            raise KeyError(str(response))
        for flag, code in [("0", "session"), ("1", "request")]:
            try:
                get_account_info(flag)
            except KeyError as error:
                self.assertEqual(request_failure(error), {"type": "error", "code": code})

    def test_reconnect_preserves_selected_account_and_classifies_transport_failure(self):
        with patch("youtube_music.selected_zen_database", return_value=Path("fixture")), \
                patch("youtube_music.read_cookie_database", return_value=[
                    {"name": "__Secure-3PAPISID", "value": "fixture", "domain": ".youtube.com"}]), \
                patch("youtube_music.validate_session", return_value=True) as validate:
            result = connect_existing("fixture", 1)
            self.assertEqual(result["headers"]["x-goog-authuser"], "1")
            validate.assert_called_once_with(result["headers"])
            validate.reset_mock()
            for invalid in [-1, 10, True, "1"]:
                self.assertEqual(connect_existing("fixture", invalid), {"type": "error", "code": "session"})
            validate.assert_not_called()
            validate.side_effect = TimeoutError("private")
            self.assertEqual(connect_existing("fixture", 1), {"type": "error", "code": "request"})
    def test_temporary_service_failures_keep_credentials_out_of_results(self):
        for status in [429, 500, 503, None]:
            error = RuntimeError("private fixture must never appear in output")
            error.response = SimpleNamespace(status_code=status)
            result = request_failure(error)
            self.assertEqual(result, {"type": "error", "code": "request"})
            self.assertNotIn("private fixture", json.dumps(result))

    def test_radio_filters_seed_duplicates_malformed_and_unavailable_tracks(self):
        client = FakeClient()
        items = [None, "unsupported", {}, {"videoId": "bad/id"}, {"videoId": 12345678901},
                 {"videoId": "K4DyBUG242c", "title": "Seed"},
                 {"videoId": "00000000001", "title": "Unavailable", "isAvailable": False},
                 {"videoId": "00000000002", "title": "First", "artists": [None, {}, {"name": "Artist"}]},
                 {"videoId": "00000000002", "title": "Duplicate"},
                 {"videoId": "00000000003", "title": "Second", "artists": "unsupported", "album": "unsupported"}]
        client.get_watch_playlist = MagicMock(return_value={"tracks": items})
        result = run(client, {"action": "radio", "video_id": "K4DyBUG242c"})
        client.get_watch_playlist.assert_called_once_with(videoId="K4DyBUG242c", limit=25, radio=True)
        self.assertEqual([track["title"] for track in result["tracks"]], ["First", "Second"])
        self.assertEqual(result["tracks"][0]["artist"], "Artist")
        self.assertEqual(result["tracks"][1]["artist"], "")
        self.assertEqual(result["tracks"][1]["album"], "")
        self.assertEqual(result["tracks"][0]["source"], "https://www.youtube.com/watch?v=00000000002")
        self.assertFalse(result["truncated"])

    def test_radio_caps_api_overshoot_and_only_uses_read_endpoint(self):
        client = SimpleNamespace(get_watch_playlist=MagicMock(return_value={"tracks": [
            {"videoId": f"{i:011}", "title": f"Track {i}"} for i in range(1, 201)]}))
        result = run(client, {"action": "radio", "video_id": "K4DyBUG242c"})
        self.assertEqual(len(result["tracks"]), MAX_RADIO_TRACKS)
        self.assertEqual(result["tracks"][-1]["title"], "Track 25")
        self.assertTrue(result["truncated"])
        client.get_watch_playlist.assert_called_once()

    def test_radio_empty_and_failures_are_bounded_generic_results(self):
        client = SimpleNamespace(get_watch_playlist=MagicMock())
        empty = {"type": "tracks", "tracks": [], "truncated": False}
        for invalid in [None, 12345678901, "bad", "https://other.test"]:
            self.assertEqual(run(client, {"action": "radio", "video_id": invalid}), empty)
        client.get_watch_playlist.assert_not_called()
        for malformed in [None, [], {}, {"tracks": None}, {"tracks": "unsupported"}, {"tracks": []}]:
            client.get_watch_playlist.return_value = malformed
            self.assertEqual(run(client, {"action": "radio", "video_id": "K4DyBUG242c"}), empty)
        client.get_watch_playlist.side_effect = ValueError("private headers must not be returned")
        self.assertEqual(run(client, {"action": "radio", "video_id": "K4DyBUG242c"}),
                         {"type": "error", "code": "request"})
        for status in [401, 403]:
            failure = RuntimeError("private headers must not be returned")
            failure.response = SimpleNamespace(status_code=status)
            client.get_watch_playlist.side_effect = failure
            self.assertEqual(run(client, {"action": "radio", "video_id": "K4DyBUG242c"}),
                             {"type": "error", "code": "session"})
            client.get_watch_playlist.side_effect = RuntimeError(f"Server returned HTTP {status}: private fixture")
            self.assertEqual(run(client, {"action": "radio", "video_id": "K4DyBUG242c"}),
                             {"type": "error", "code": "session"})

    def test_artwork_chooses_small_safe_public_thumbnail_and_watch_variant(self):
        candidates = [{"url": "https://i.ytimg.com/large.jpg", "width": 512, "height": 512},
                      {"url": "https://lh3.googleusercontent.com/small", "width": 96, "height": 96},
                      {"url": "https://yt3.ggpht.com/exact", "width": 120, "height": 120}]
        self.assertEqual(artwork_url({"thumbnails": candidates}), "https://yt3.ggpht.com/exact")
        self.assertEqual(artwork_url({"thumbnail": candidates[:2]}), "https://lh3.googleusercontent.com/small")
        self.assertEqual(tracks([{"videoId": "K4DyBUG242c", "thumbnail": candidates}])[0]["artwork_url"],
                         "https://yt3.ggpht.com/exact")
        client = FakeClient()
        client.get_library_playlists = lambda limit: [{"playlistId": "PLfixture", "thumbnails": candidates}]
        client.get_home = lambda limit: [{"contents": [{"videoId": "K4DyBUG242c", "thumbnails": candidates}]}]
        self.assertEqual(run(client, {"action": "library"})["playlists"][0]["artwork_url"], "https://yt3.ggpht.com/exact")
        self.assertEqual(run(client, {"action": "home"})["cards"][0]["artwork_url"], "https://yt3.ggpht.com/exact")

    def test_artwork_rejects_unsafe_urls_and_malformed_thumbnails(self):
        for url in ["http://i.ytimg.com/a", "https://i.ytimg.com.evil.test/a", "https://evilytimg.com/a",
                    "https://user:password@i.ytimg.com/a", "https://127.0.0.1/a", "https://i.ytimg.com:8080/a",
                    "https://i.ytimg.com:bad/a", "https://i.ytimg.com/a\nheader", "https://[bad/a",
                    "https://i.ytimg.com/" + "x" * 2048]:
            self.assertIsNone(artwork_url({"thumbnails": [{"url": url, "width": 120, "height": 120}]}))
        for thumbnails in [None, {}, "unsupported", [None, "unsupported", {}]]:
            self.assertIsNone(artwork_url({"thumbnails": thumbnails}))
        self.assertIsNone(artwork_url({"thumbnails": [{}] * 32 + [{"url": "https://i.ytimg.com/a"}]}))

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

    def test_home_handles_mixed_package_parser_output(self):
        client = FakeClient()
        client.get_home = lambda limit: [
            {"title": "Notice", "contents": "An informational description"},
            {"title": "Listen again", "contents": [None, {}, "unsupported",
                {"title": "Album", "browseId": "MPREfixture", "type": "Album", "artists": None},
                {"title": "Artist", "browseId": "UCfixture"},
                {"title": "Mix", "playlistId": "PLfixture", "author": [{"name": "Fixture author"}]},
                {"title": "Song", "videoId": "K4DyBUG242c", "artists": [None, {"name": None}]}]},
        ]
        result = run(client, {"action": "home"})
        self.assertEqual([card["kind"] for card in result["cards"]], ["album", "artist", "playlist", "song"])
        self.assertEqual(result["cards"][2]["subtitle"], "Fixture author")
        self.assertEqual(result["cards"][3]["subtitle"], "")


class LoginTests(unittest.TestCase):
    COOKIE = {"name": "__Secure-3PAPISID", "value": "fixture", "domain": ".youtube.com", "path": "/"}

    def database(self, root, origin_attributes=True):
        profile = Path(tempfile.mkdtemp(prefix="zen-login-", dir=root))
        database = profile / "cookies.sqlite"
        with closing(sqlite3.connect(database)) as connection, connection:
            extra = ", originAttributes TEXT DEFAULT ''" if origin_attributes else ""
            connection.execute("CREATE TABLE moz_cookies (name TEXT, value TEXT, host TEXT, path TEXT, expiry INTEGER" + extra + ")")
        return profile

    def insert(self, profile, name="__Secure-3PAPISID", value="fixture", host=".youtube.com", path="/", expiry=None, attrs=None):
        row = [name, value, host, path, expiry if expiry is not None else int(time.time()) + 3600]
        if attrs is not None:
            row.append(attrs)
        with closing(sqlite3.connect(profile / "cookies.sqlite")) as connection, connection:
            connection.execute("INSERT INTO moz_cookies VALUES (" + ",".join("?" for _ in row) + ")", row)

    def test_sqlite_reads_only_scoped_unexpired_default_context(self):
        with tempfile.TemporaryDirectory() as root:
            profile = self.database(root)
            for host in [".youtube.com", "youtube.com", "music.youtube.com", ".music.youtube.com"]:
                self.insert(profile, host=host, attrs="")
            self.insert(profile, name="GoogleSecret", host=".google.com", attrs="")
            self.insert(profile, name="LookalikeSecret", host=".youtube.com.evil.test", attrs="")
            self.insert(profile, name="WrongPath", path="/watch", attrs="")
            self.insert(profile, name="Expired", expiry=int(time.time()) - 1, attrs="")
            self.insert(profile, name="ContainerSecret", attrs="^userContextId=1")
            self.insert(profile, name="Oversize", value="x" * 4097, attrs="")
            cookies = read_music_cookies(profile, Path(root))
            self.assertEqual(len(cookies), 4)
            self.assertEqual({cookie["name"] for cookie in cookies}, {"__Secure-3PAPISID"})
            self.assertEqual({cookie["domain"] for cookie in cookies},
                             {".youtube.com", "youtube.com", "music.youtube.com", ".music.youtube.com"})
            real_connect = sqlite3.connect
            observed = []
            def readonly_connect(database_uri, **kwargs):
                observed.append((database_uri, kwargs))
                return real_connect(database_uri, **kwargs)
            with patch("youtube_music.sqlite3.connect", side_effect=readonly_connect):
                self.assertEqual(len(read_music_cookies(profile, Path(root))), 4)
            self.assertTrue(observed[0][0].endswith("?mode=ro"))
            self.assertTrue(observed[0][1]["uri"])
            self.assertLessEqual(observed[0][1]["timeout"], 0.1)

    def test_strict_profile_read_distinguishes_lock_and_container_without_mixing_sessions(self):
        with tempfile.TemporaryDirectory() as root:
            profile = self.database(root)
            database = profile / "cookies.sqlite"
            self.insert(profile, value="container-private-fixture", attrs="^userContextId=1")
            with self.assertRaisesRegex(CookieReadError, "^container$"):
                read_cookie_database(database, strict=True)
            self.insert(profile, value="default-fixture", attrs="")
            cookies = read_cookie_database(database, strict=True)
            self.assertEqual([cookie["value"] for cookie in cookies], ["default-fixture"])
            with closing(sqlite3.connect(database)) as lock:
                lock.execute("BEGIN EXCLUSIVE")
                with self.assertRaisesRegex(CookieReadError, "^profile_read$"):
                    read_cookie_database(database, strict=True)
                lock.rollback()

    def test_sqlite_old_schema_row_limit_and_missing_database(self):
        with tempfile.TemporaryDirectory() as root:
            profile = self.database(root, origin_attributes=False)
            self.insert(profile)
            self.assertEqual(len(read_music_cookies(profile, Path(root))), 1)
            for index in range(200):
                self.insert(profile, name="Cookie" + str(index))
            self.assertEqual(read_music_cookies(profile, Path(root)), [])
            missing = Path(tempfile.mkdtemp(prefix="zen-login-", dir=root))
            self.assertEqual(read_music_cookies(missing, Path(root)), [])
            self.assertFalse((missing / "cookies.sqlite").exists())
            self.assertEqual(read_music_cookies(profile, missing), [])

    def test_normalizes_selected_headers_and_rejects_invalid_session(self):
        headers = session_headers({"Cookie": "__Secure-3PAPISID=fixture", "X-Goog-AuthUser": "0",
                                   "X-Goog-PageId": "123", "Origin": "https://wrong.test",
                                   "Authorization": "SAPISIDHASH fixture", "X-Other-Secret": "fixture"})
        self.assertEqual(headers["origin"], MUSIC_URL)
        self.assertEqual(headers["x-goog-pageid"], "123")
        self.assertNotIn("authorization", headers)
        self.assertNotIn("x-other-secret", headers)
        self.assertIsNone(session_headers({"x-goog-authuser": "0"}, [{**self.COOKIE, "domain": ".google.com"}]))
        self.assertIsNone(session_headers({"x-goog-authuser": "0"}, [{**self.COOKIE, "value": "x" * 4097}]))
        self.assertIsNone(session_headers({"cookie": "SID=fixture", "x-goog-authuser": "0"}))
        self.assertIsNone(session_headers({"x-goog-authuser": "0\r\nx:bad"}, [self.COOKIE]))
        self.assertEqual(music_cookies([{**self.COOKIE, "name": "C" + str(i), "value": "x" * 4096}
                                       for i in range(20)]), "")

    def test_wait_requires_account_validation_and_limits_retries(self):
        clock = [0]
        process = SimpleNamespace(poll=lambda: None)
        def sleep(seconds):
            clock[0] += seconds
        with patch("youtube_music.time.monotonic", side_effect=lambda: clock[0]), \
                patch("youtube_music.time.sleep", side_effect=sleep), \
                patch("youtube_music.read_music_cookies", return_value=[self.COOKIE]), \
                patch("youtube_music.validate_session", side_effect=[False, True]) as validate:
            result = wait_for_login(process, Path("fixture"), Path("fixture"), timeout=30)
        self.assertEqual(result["type"], "session")
        self.assertEqual(result["headers"]["x-goog-authuser"], "0")
        self.assertEqual(validate.call_count, 2)
        self.assertGreaterEqual(clock[0], 15)

    def test_wait_closed_and_eight_minute_bound_without_credentials(self):
        clock = [0]
        process = SimpleNamespace(poll=lambda: None)
        def sleep(seconds):
            clock[0] += seconds
        with patch("youtube_music.time.monotonic", side_effect=lambda: clock[0]), \
                patch("youtube_music.time.sleep", side_effect=sleep), \
                patch("youtube_music.read_music_cookies", return_value=[]), \
                patch("youtube_music.validate_session") as validate:
            self.assertEqual(wait_for_login(process, Path("fixture"), Path("fixture"), timeout=99999)["code"], "login_timeout")
        self.assertEqual(clock[0], LOGIN_SECONDS)
        validate.assert_not_called()
        process.poll = lambda: 0
        self.assertEqual(wait_for_login(process, Path("fixture"), Path("fixture"))["code"], "login_closed")

    def test_identity_validation_closes_session_and_requires_real_name(self):
        for info, expected in [({"accountName": "Fixture listener"}, True), ({"name": "Fixture"}, True),
                               ({}, False), ({"accountName": {}}, False), ({"accountName": ""}, False), (None, False)]:
            client, session = MagicMock(), MagicMock()
            client.get_account_info.return_value = info
            with patch("youtube_music.create_client", return_value=(client, session)):
                self.assertEqual(validate_session({}), expected)
            session.close.assert_called_once()

    def test_client_sets_browser_auth_marker_and_network_timeout(self):
        api = MagicMock()
        requests = MagicMock()
        helpers = SimpleNamespace(initialize_headers=lambda: {"origin": MUSIC_URL})
        with patch.dict("sys.modules", {"ytmusicapi": api, "ytmusicapi.helpers": helpers, "requests": requests}):
            create_client({"cookie": "__Secure-3PAPISID=fixture", "x-goog-authuser": "0"})
        options = api.YTMusic.call_args.kwargs
        self.assertEqual(options["auth"]["authorization"], "SAPISIDHASH")
        self.assertEqual(requests.Session.return_value.request.keywords["timeout"], 20)

    def test_removal_only_deletes_owned_child_profile(self):
        with tempfile.TemporaryDirectory() as private, tempfile.TemporaryDirectory() as unrelated:
            profile = Path(tempfile.mkdtemp(prefix="zen-login-", dir=private))
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
                patch("youtube_music.connect", return_value={"type": "error", "code": "login_closed"}) as handler:
            main()
        handler.assert_called_once_with("fixture")
        self.assertEqual(json.loads(stdout.getvalue()), {"type": "error", "code": "login_closed"})

    def test_connect_opens_normal_fresh_profile_and_preserves_session_on_locks(self):
        expected = {"type": "session", "headers": {"cookie": "fixture"}}
        with tempfile.TemporaryDirectory() as private, \
                patch("youtube_music.zen_executable", return_value="fixture/zen.exe"), \
                patch("youtube_music.subprocess.Popen") as popen, \
                patch("youtube_music.close_login_browser") as close, \
                patch("youtube_music.remove_login_profile", return_value=False):
            def signed_in(process, profile, directory):
                self.assertEqual(profile.parent, Path(private).resolve())
                self.assertTrue(profile.name.startswith("zen-login-"))
                self.assertEqual((profile / "user.js").read_text(), 'user_pref("signon.rememberSignons", false);\n')
                return expected
            with patch("youtube_music.wait_for_login", side_effect=signed_in):
                self.assertEqual(connect(private), expected)
            args = popen.call_args.args[0]
            self.assertEqual(args[1:4], ["-no-remote", "-wait-for-browser", "-profile"])
            self.assertEqual(args[-1], MUSIC_URL)
            self.assertEqual(set(popen.call_args.kwargs), {"stdout", "stderr"})
            close.assert_called_once_with(popen.return_value)

    def test_executable_discovery_uses_zen_override(self):
        with tempfile.TemporaryDirectory() as root:
            executable = Path(root) / "zen.exe"
            executable.touch()
            with patch.dict("youtube_music.os.environ", {"TIDE_ZEN_EXE": str(executable)}):
                self.assertEqual(zen_executable(), str(executable.resolve()))

    def test_selected_database_is_confined_to_explicit_zen_profiles_file(self):
        with tempfile.TemporaryDirectory() as appdata, tempfile.TemporaryDirectory() as unrelated, \
                patch.dict("youtube_music.os.environ", {"APPDATA": appdata}):
            root = Path(appdata) / "zen" / "Profiles"
            root.mkdir(parents=True)
            profile = self.database(root)
            selected = profile / "cookies.sqlite"
            self.assertEqual(selected_zen_database(str(selected)), selected.resolve())
            outside = self.database(unrelated) / "cookies.sqlite"
            wrong_name = profile / "logins.json"
            wrong_name.write_text("fixture")
            not_a_file = root / "cookies.sqlite"
            not_a_file.mkdir()
            for invalid in [outside, wrong_name, not_a_file, Path("cookies.sqlite"), profile / "missing.sqlite"]:
                with patch("youtube_music.read_cookie_database") as reader:
                    self.assertEqual(connect_existing(str(invalid)), {"type": "error", "code": "profile"})
                reader.assert_not_called()
            with patch("youtube_music.Path.is_symlink", return_value=True):
                self.assertEqual(connect_existing(str(selected)), {"type": "error", "code": "profile"})

    @unittest.skipUnless(os.name == "nt", "Windows file path aliases")
    def test_selected_database_accepts_windows_short_path_alias(self):
        import ctypes
        with tempfile.TemporaryDirectory() as appdata, \
                patch.dict("youtube_music.os.environ", {"APPDATA": appdata}):
            root = Path(appdata) / "zen" / "Profiles"
            root.mkdir(parents=True)
            selected = self.database(root) / "cookies.sqlite"
            buffer = ctypes.create_unicode_buffer(4096)
            length = ctypes.windll.kernel32.GetShortPathNameW(str(selected.parent), buffer, len(buffer))
            self.assertGreater(length, 0)
            alias = Path(buffer.value) / "cookies.sqlite"
            self.assertEqual(selected_zen_database(str(alias)), selected.resolve())

    def test_existing_connection_verifies_account_without_browser_or_profile_changes(self):
        with tempfile.TemporaryDirectory() as appdata, \
                patch.dict("youtube_music.os.environ", {"APPDATA": appdata}), \
                patch("youtube_music.subprocess.Popen") as spawn, \
                patch("youtube_music.close_login_browser") as close, \
                patch("youtube_music.remove_login_profile") as remove:
            root = Path(appdata) / "zen" / "Profiles"
            root.mkdir(parents=True)
            profile = self.database(root)
            self.insert(profile, attrs="")
            selected = profile / "cookies.sqlite"
            before = selected.read_bytes()
            with patch("youtube_music.validate_session", return_value=True) as validate:
                result = connect_existing(str(selected))
            self.assertEqual(result["type"], "session")
            self.assertEqual(result["headers"]["x-goog-authuser"], "0")
            validate.assert_called_once_with(result["headers"])
            for failure, code in [(False, "session"), (ValueError("fixture error"), "request")]:
                with patch("youtube_music.validate_session", side_effect=[failure]):
                    self.assertEqual(connect_existing(str(selected)), {"type": "error", "code": code})
            self.assertEqual(selected.read_bytes(), before)
            self.assertEqual([path.name for path in profile.iterdir()], ["cookies.sqlite"])
            spawn.assert_not_called()
            close.assert_not_called()
            remove.assert_not_called()

    def test_existing_connect_dispatches_private_payload(self):
        stdin = SimpleNamespace(buffer=io.BytesIO(b'{"action":"connect_existing","database":"fixture"}'))
        stdout = io.StringIO()
        with patch("youtube_music.sys.stdin", stdin), patch("youtube_music.sys.stdout", stdout), \
                patch("youtube_music.connect_existing", return_value={"type": "error", "code": "profile"}) as handler:
            main()
        handler.assert_called_once_with("fixture", 0)
        self.assertEqual(json.loads(stdout.getvalue()), {"type": "error", "code": "profile"})

    def test_account_switch_verifies_zero_and_one_without_mutating_current_headers(self):
        current = {"cookie": "__Secure-3PAPISID=fixture", "x-goog-authuser": "0"}
        for index in [0, 1]:
            with patch("youtube_music.validate_session", return_value=True) as validate:
                result = switch_account(current, index)
            self.assertEqual(result["type"], "session")
            self.assertEqual(result["headers"]["x-goog-authuser"], str(index))
            validate.assert_called_once_with(result["headers"])
        self.assertEqual(current, {"cookie": "__Secure-3PAPISID=fixture", "x-goog-authuser": "0"})

    def test_account_switch_rejects_invalid_indices_and_failed_accounts(self):
        current = {"cookie": "__Secure-3PAPISID=fixture", "x-goog-authuser": "0"}
        for index in [-1, 10, 9999, True, False, 1.0, "1", None]:
            with patch("youtube_music.validate_session") as validate:
                self.assertEqual(switch_account(current, index), {"type": "error", "code": "session"})
            validate.assert_not_called()
        for failure, code in [(False, "session"), (ValueError("fixture error"), "request")]:
            with patch("youtube_music.validate_session", side_effect=[failure]):
                self.assertEqual(switch_account(current, 1), {"type": "error", "code": code})
        self.assertEqual(current["x-goog-authuser"], "0")
        self.assertEqual(switch_account({"cookie": "invalid"}, 1), {"type": "error", "code": "session"})

    def test_account_switch_drops_inherited_brand_channel_before_verification(self):
        for channel_header in ["x-goog-pageid", "X-Goog-PageId"]:
            current = {"cookie": "__Secure-3PAPISID=fixture", "x-goog-authuser": "0", channel_header: "123"}
            def validate(candidate):
                self.assertNotIn("x-goog-pageid", candidate)
                self.assertEqual(candidate["x-goog-authuser"], "1")
                return True
            with patch("youtube_music.validate_session", side_effect=validate):
                result = switch_account(current, 1)
            self.assertEqual(result["type"], "session")
            self.assertNotIn("x-goog-pageid", result["headers"])
            self.assertEqual(current[channel_header], "123")

    def test_account_switch_dispatches_private_payload_before_client_creation(self):
        stdin = SimpleNamespace(buffer=io.BytesIO(b'{"action":"switchaccount","headers":{},"index":1}'))
        stdout = io.StringIO()
        with patch("youtube_music.sys.stdin", stdin), patch("youtube_music.sys.stdout", stdout), \
                patch("youtube_music.switch_account", return_value={"type": "error", "code": "session"}) as handler, \
                patch("youtube_music.create_client") as create:
            main()
        handler.assert_called_once_with({}, 1)
        create.assert_not_called()
        self.assertEqual(json.loads(stdout.getvalue()), {"type": "error", "code": "session"})


if __name__ == "__main__":
    unittest.main()

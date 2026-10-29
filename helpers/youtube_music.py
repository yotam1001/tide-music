"""YouTube Music bridge. Credentials arrive on stdin, never argv.

Collection writes require a separate explicit, strictly validated feedback action.
Ordinary browsing, radio and automated playback probes remain read-only.
"""
import json
from contextlib import closing
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import warnings
from urllib.parse import urlsplit

MAX_INPUT = 65536
MAX_TRACKS = 2000
MAX_RADIO_TRACKS = 25
MAX_HOME_SHELVES = 3
MAX_HOME_ITEMS_PER_SHELF = 40
MAX_HOME_CARDS = MAX_HOME_SHELVES * MAX_HOME_ITEMS_PER_SHELF
MAX_SUPERMIX_SHELVES = 20
ARTWORK_HOSTS = ("ytimg.com", "ggpht.com", "googleusercontent.com")
LOGIN_SECONDS = 8 * 60
LOGIN_RETRY_SECONDS = 15
MUSIC_URL = "https://music.youtube.com"


def request_failure(error):
    """Keep authentication failure distinct from a temporary service failure."""
    response = getattr(error, "response", None)
    status = getattr(response, "status_code", None) or getattr(error, "status_code", None)
    message = error.args[0] if error.args and isinstance(error.args[0], str) else ""
    expired = status in (401, 403) or re.match(r"Server returned HTTP (401|403):", message[:32])
    # The API can return HTTP 200 with an anonymous account menu. The pinned
    # client's parser then raises KeyError containing the full response. Inspect
    # only the fixed authentication flag; never return exception text or data.
    tb = error.__traceback__
    while tb:
        if tb.tb_frame.f_code.co_name == "get_account_info":
            payload = tb.tb_frame.f_locals.get("response")
            if isinstance(payload, dict):
                context = payload.get("responseContext", {})
                if isinstance(context, dict):
                    services = context.get("serviceTrackingParams", [])
                    for service in services if isinstance(services, list) else []:
                        params = service.get("params", []) if isinstance(service, dict) else []
                        for param in params if isinstance(params, list) else []:
                            if isinstance(param, dict) and param.get("key") == "logged_in" and param.get("value") == "0":
                                expired = True
        tb = tb.tb_next
    return {"type": "error", "code": "session" if expired else "request"}


def verified_request_failure(error, client):
    """A content endpoint denial alone does not prove the account expired."""
    result = request_failure(error)
    if result["code"] != "session":
        return result
    try:
        info = client.get_account_info()
        name = (info.get("accountName") or info.get("name")) if isinstance(info, dict) else None
        if isinstance(name, str) and clean(name):
            return {"type": "error", "code": "request"}
    except Exception as verification_error:
        # Only an identity-endpoint rejection confirms a bad session. A timeout,
        # outage or changed response shape must not erase a working connection.
        return request_failure(verification_error)
    return {"type": "error", "code": "request"}

# Firefox stores domain cookies with a leading dot, host-only cookies without it.
# A host-only youtube.com cookie is not sent to music.youtube.com.
COOKIE_DOMAINS = (".youtube.com", "music.youtube.com", ".music.youtube.com")
SESSION_HEADERS = {
    "accept": 1024, "accept-language": 1024, "content-type": 256,
    "user-agent": 1024, "x-goog-authuser": 8, "x-goog-pageid": 128,
    "cookie": 32768, "x-goog-visitor-id": 4096,
}
warnings.filterwarnings("ignore")


def safe_header(value, limit):
    return (isinstance(value, str) and bool(value) and len(value) <= limit
            and all(32 <= ord(char) < 127 or char == "\t" for char in value))


class CookieSelectionError(Exception):
    pass


def cookie_matches_music(domain, path="/", domain_specified=None, request_path="/"):
    """RFC domain/path matching; the shared header uses the Music origin path."""
    if not isinstance(domain, str) or not isinstance(path, str) or not path.startswith("/"):
        return False
    host = "music.youtube.com"
    bare_domain = (domain[1:] if domain.startswith(".") else domain).lower()
    if bare_domain not in ("youtube.com", host):
        return False
    is_domain = domain.startswith(".") if domain_specified is None else domain_specified
    domain_matches = (host == bare_domain or (is_domain and host.endswith("." + bare_domain)))
    path_matches = (request_path == path or (request_path.startswith(path)
                    and (path.endswith("/") or request_path[len(path):].startswith("/"))))
    return domain_matches and path_matches


def authentication_cookie(name):
    return (name.startswith("__Secure-")
            or name in {"SID", "HSID", "SSID", "APISID", "SAPISID", "SIDCC", "LOGIN_INFO"})


def music_cookies(cookies):
    """Normalize only caller-authorized YouTube cookies."""
    result = []
    seen = {}
    for cookie in cookies[:200]:
        if not cookie_matches_music(cookie.get("domain"), cookie.get("path", "/")):
            continue
        name, value = cookie.get("name", ""), cookie.get("value", "")
        if (re.fullmatch(r"[A-Za-z0-9_-]{1,128}", name)
                and safe_header(value, 4096) and ";" not in value):
            if name in seen:
                if value in seen[name]:
                    continue
                # Scope alone cannot establish which conflicting account cookie
                # the service will use. Require a captured successful request.
                if authentication_cookie(name):
                    raise CookieSelectionError("cookie_ambiguity")
            seen.setdefault(name, set()).add(value)
            result.append(name + "=" + value)
    value = "; ".join(result)
    return value if len(value) <= SESSION_HEADERS["cookie"] else ""


def session_headers(raw, cookies=()):
    """Keep only YTMusic inputs; transient Authorization is generated by ytmusicapi."""
    headers = {}
    for name, value in raw.items():
        key = name.lower()
        if key in SESSION_HEADERS and safe_header(value, SESSION_HEADERS[key]):
            headers[key] = value
    if not headers.get("cookie"):
        headers["cookie"] = music_cookies(cookies)
    authuser = headers.get("x-goog-authuser", "")
    if not re.fullmatch(r"[0-9]{1,4}", authuser):
        return None
    cookie_values = dict(part.strip().split("=", 1) for part in headers["cookie"].split(";")
                         if "=" in part)
    if not cookie_values.get("__Secure-3PAPISID"):
        return None
    if "x-goog-pageid" in headers and not re.fullmatch(r"[0-9]{1,128}", headers["x-goog-pageid"]):
        return None
    headers["origin"] = MUSIC_URL
    return headers


def owned_profile(profile, directory):
    resolved = profile.resolve()
    return (resolved.parent == directory.resolve()
            and resolved.name.startswith("zen-login-") and not profile.is_symlink())


def read_music_cookies(profile, directory):
    """Read only this helper's own SQLite database, never a user's browser profile."""
    if not owned_profile(profile, directory):
        return []
    database = profile / "cookies.sqlite"
    if not database.is_file() or database.is_symlink() or database.resolve().parent != profile.resolve():
        return []
    return read_cookie_database(database)


class CookieReadError(Exception):
    pass


def read_cookie_database(database, strict=False):
    """Caller must authorize and confine the file before this bounded read."""
    try:
        with closing(sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True, timeout=0.1)) as connection:
            connection.execute("PRAGMA query_only=ON")
            columns = {row[1] for row in connection.execute("PRAGMA table_info(moz_cookies)")}
            context_filter = " AND originAttributes = ''" if "originAttributes" in columns else ""
            # All selected cookies have the same path. RFC ordering therefore
            # uses creation time, never domain specificity. Old schemas lack it.
            order = "creationTime, rowid" if "creationTime" in columns else "rowid"
            rows = connection.execute(
                "SELECT name, value, host, path FROM moz_cookies "
                "WHERE host IN (?, ?, ?) AND path = '/' AND expiry > ? "
                "AND length(name) BETWEEN 1 AND 128 AND length(value) BETWEEN 1 AND 4096"
                + context_filter + " ORDER BY " + order + " LIMIT 201",
                (*COOKIE_DOMAINS, int(time.time())),
            ).fetchall()
            if strict and "originAttributes" in columns and not any(row[0] == "__Secure-3PAPISID" for row in rows):
                container = connection.execute(
                    "SELECT 1 FROM moz_cookies WHERE host IN (?, ?, ?) "
                    "AND path = '/' AND name = '__Secure-3PAPISID' AND originAttributes != '' AND expiry > ? LIMIT 1",
                    (*COOKIE_DOMAINS, int(time.time())),
                ).fetchone()
                if container:
                    raise CookieReadError("container")
        if len(rows) > 200:
            return []
        return [{"name": name, "value": value, "domain": host, "path": path}
                for name, value, host, path in rows]
    except (sqlite3.Error, OSError):
        # Browser startup, write locks, or shutdown can temporarily prevent a read.
        if strict:
            raise CookieReadError("profile_read") from None
        return []


def selected_zen_database(value):
    database = Path(value)
    if not database.is_absolute() or database.name != "cookies.sqlite" or database.is_symlink():
        raise ValueError("invalid selected profile")
    canonical = database.resolve(strict=True)
    root = (Path(os.environ["APPDATA"]) / "zen" / "Profiles").resolve(strict=True)
    # Windows file pickers/TEMP can use harmless 8.3 aliases. Enforce the resolved
    # boundary rather than requiring the spelling to already be canonical.
    if not canonical.is_relative_to(root) or not canonical.is_file():
        raise ValueError("invalid selected profile")
    return canonical


def connect_existing(database, index=0):
    # This path arrives only after the user explicitly selects the cookie file.
    # No browser is opened, no profile is copied, and no files are changed here.
    try:
        database = selected_zen_database(database)
    except (ValueError, OSError, KeyError, TypeError):
        return {"type": "error", "code": "profile"}
    if type(index) is not int or not 0 <= index <= 9:
        return {"type": "error", "code": "session"}
    try:
        cookies = read_cookie_database(database, strict=True)
        headers = session_headers({"x-goog-authuser": str(index)}, cookies)
    except (CookieReadError, CookieSelectionError) as error:
        return {"type": "error", "code": error.args[0]}
    if not headers:
        return {"type": "error", "code": "session"}
    try:
        if validate_session(headers):
            return {"type": "session", "headers": headers}
    except ImportError:
        return {"type": "error", "code": "runtime"}
    except Exception as error:
        return request_failure(error)
    return {"type": "error", "code": "session"}


def switch_account(headers, index):
    if type(index) is not int or not 0 <= index <= 9 or not isinstance(headers, dict):
        return {"type": "error", "code": "session"}
    try:
        # Normalize a separate candidate. Failed verification must leave the
        # caller's current session unchanged and must not return new headers.
        candidate_source = {name: value for name, value in headers.items()
                            if name.lower() != "x-goog-pageid"}
        candidate = session_headers({**candidate_source, "x-goog-authuser": str(index)})
        if candidate and validate_session(candidate):
            return {"type": "session", "headers": candidate}
    except ImportError:
        return {"type": "error", "code": "runtime"}
    except Exception as error:
        return request_failure(error)
    return {"type": "error", "code": "session"}


def create_client(auth):
    from ytmusicapi import YTMusic
    from ytmusicapi.helpers import initialize_headers
    import requests
    from functools import partial
    session = requests.Session()
    transport = session.request

    def contained_request(*args, **kwargs):
        # Session.get supplies allow_redirects=True itself, which would override
        # a partial's default. Enforce the boundary at the final request call.
        kwargs["allow_redirects"] = False
        kwargs["timeout"] = 20
        return transport(*args, **kwargs)

    session.request = partial(contained_request, timeout=20)
    headers = initialize_headers()
    headers.update(auth)
    # Avoid ytmusicapi's unrelated anonymous webpage bootstrap. Captured browser
    # visitor data is preserved; the authenticated API does not require a visit
    # to the browser page when no visitor value was captured.
    headers.setdefault("x-goog-visitor-id", "")
    current = dict(auth)
    holder = []
    session._tide_refreshed_headers = None

    def refresh_cookies(response, *args, **kwargs):
        if urlsplit(response.url).hostname not in ("music.youtube.com", "www.youtube.com", "youtube.com"):
            return
        values = dict(part.strip().split("=", 1) for part in current.get("cookie", "").split(";") if "=" in part)
        changed = False
        received_auth = {}
        for cookie in response.cookies:
            if not cookie_matches_music(cookie.domain, cookie.path,
                                        getattr(cookie, "domain_specified", None)):
                continue
            if not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", cookie.name):
                continue
            if cookie.expires is not None and cookie.expires <= time.time():
                changed |= cookie.name in values
                values.pop(cookie.name, None)
            elif safe_header(cookie.value, 4096) and ";" not in cookie.value:
                if authentication_cookie(cookie.name):
                    if cookie.name in received_auth and received_auth[cookie.name] != cookie.value:
                        # An ambiguous response must not replace the known session.
                        return
                    received_auth[cookie.name] = cookie.value
                changed |= values.get(cookie.name) != cookie.value
                values[cookie.name] = cookie.value
        candidate = session_headers({**current, "cookie": "; ".join(name + "=" + value for name, value in values.items())})
        if changed and candidate:
            current.clear()
            current.update(candidate)
            session._tide_refreshed_headers = candidate.copy()
            if holder:
                holder[0].base_headers["cookie"] = candidate["cookie"]
                # ytmusicapi caches the signing cookie during construction.
                holder[0].sapisid = values["__Secure-3PAPISID"]

    session.hooks["response"].append(refresh_cookies)
    # This marker selects browser authentication. ytmusicapi generates the actual
    # current-time signature from __Secure-3PAPISID before sending any request.
    headers["authorization"] = "SAPISIDHASH"
    try:
        client = YTMusic(auth=headers, user=headers.get("x-goog-pageid"), requests_session=session)
        holder.append(client)
        return client, session
    except Exception:
        session.close()
        raise


def validate_session(headers):
    client, session = create_client(headers)
    try:
        info = client.get_account_info()
        name = (info.get("accountName") or info.get("name")) if isinstance(info, dict) else None
        verified = isinstance(name, str) and bool(clean(name))
        refreshed = getattr(session, "_tide_refreshed_headers", None)
        if verified and isinstance(refreshed, dict):
            candidate = session_headers(refreshed)
            if (candidate and candidate.get("x-goog-authuser") == headers.get("x-goog-authuser")
                    and candidate.get("x-goog-pageid") == headers.get("x-goog-pageid")):
                headers.clear()
                headers.update(candidate)
        return verified
    finally:
        session.close()


def zen_executable():
    override = os.environ.get("TIDE_ZEN_EXE")
    if override:
        candidate = Path(override).resolve(strict=True)
        if candidate.is_file() and candidate.name.lower() == "zen.exe":
            return str(candidate)
        raise ValueError("invalid browser executable")
    for variable in ("ProgramFiles", "ProgramFiles(x86)"):
        root = os.environ.get(variable)
        if root:
            candidate = Path(root) / "Zen Browser" / "zen.exe"
            if candidate.is_file():
                return str(candidate.resolve())
    candidate = shutil.which("zen.exe")
    if candidate:
        return str(Path(candidate).resolve())
    raise FileNotFoundError("browser unavailable")


def remove_login_profile(profile, directory):
    # Resolve immediately before every recursive delete. Never remove the account
    # directory, an old browser profile, or a path outside our private directory.
    for _ in range(20):
        resolved = profile.resolve()
        if not owned_profile(profile, directory):
            return False
        try:
            shutil.rmtree(resolved)
            return True
        except FileNotFoundError:
            return True
        except OSError:
            time.sleep(0.25)
    return False


def wait_for_login(process, profile, directory, timeout=LOGIN_SECONDS):
    deadline = time.monotonic() + min(max(timeout, 0), LOGIN_SECONDS)
    last_cookie = None
    retry_at = 0
    while time.monotonic() < deadline:
        if process.poll() is not None:
            return {"type": "error", "code": "login_closed"}
        try:
            headers = session_headers({"x-goog-authuser": "0"}, read_music_cookies(profile, directory))
        except CookieSelectionError as error:
            return {"type": "error", "code": error.args[0]}
        now = time.monotonic()
        if headers and (headers["cookie"] != last_cookie or now >= retry_at):
            last_cookie = headers["cookie"]
            retry_at = now + LOGIN_RETRY_SECONDS
            try:
                if validate_session(headers):
                    return {"type": "session", "headers": headers}
            except ImportError:
                return {"type": "error", "code": "runtime"}
            except Exception:
                # Cookie presence during a Google redirect does not prove login.
                # Network and account failures stay private and may be retried.
                pass
        time.sleep(min(1, max(0, deadline - time.monotonic())))
    return {"type": "error", "code": "login_timeout"}


def close_login_browser(process):
    if process.poll() is not None:
        return
    if os.name == "nt":
        try:
            # Without /F, taskkill requests normal window closure. /T restricts
            # closure to our own fresh-profile process and its descendants.
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=3, creationflags=subprocess.CREATE_NO_WINDOW, check=False)
            process.wait(timeout=3)
            return
        except (OSError, subprocess.TimeoutExpired):
            pass
    try:
        process.terminate()
        process.wait(timeout=3)
    except (OSError, subprocess.TimeoutExpired):
        try:
            process.kill()
            process.wait(timeout=1)
        except (OSError, subprocess.TimeoutExpired):
            pass


def connect(directory):
    directory = Path(directory).resolve(strict=True)
    if not directory.is_dir():
        raise ValueError("invalid private directory")
    profile = Path(tempfile.mkdtemp(prefix="zen-login-", dir=directory))
    result = {"type": "error", "code": "login_browser"}
    process = None
    try:
        (profile / "user.js").write_text('user_pref("signon.rememberSignons", false);\n', encoding="utf-8")
        # The normal Windows launcher waits for this separate browser instance;
        # there is no debugger, automation interface, or existing profile access.
        process = subprocess.Popen([zen_executable(), "-no-remote", "-wait-for-browser",
                                    "-profile", str(profile), MUSIC_URL],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        result = wait_for_login(process, profile, directory)
    except Exception:
        pass
    finally:
        if process is not None:
            close_login_browser(process)
        # Rust kills any remaining descendants and removes its private root after
        # helper exit. A locked file must not discard an already verified session.
        remove_login_profile(profile, directory)
    return result


def clean(value, limit=300):
    return "".join(c for c in str(value or "") if c.isprintable())[:limit]


def identifier(value, video=False):
    if not isinstance(value, str):
        return ""
    pattern = r"[A-Za-z0-9_-]{11}" if video else r"[A-Za-z0-9_-]{1,160}"
    return value if re.fullmatch(pattern, value) else ""


def artwork_url(item):
    """Choose a small public image without forwarding any account headers."""
    thumbnails = item.get("thumbnails") or item.get("thumbnail") or []
    if not isinstance(thumbnails, list):
        return None
    candidates = []
    for thumbnail in thumbnails[:32]:
        if not isinstance(thumbnail, dict):
            continue
        url = thumbnail.get("url")
        if (not isinstance(url, str) or not 1 <= len(url) <= 2048
                or any(ord(char) <= 32 or ord(char) >= 127 for char in url)):
            continue
        try:
            parsed = urlsplit(url)
            host = parsed.hostname or ""
            if (parsed.scheme != "https" or parsed.username is not None
                    or parsed.password is not None or parsed.port not in (None, 443)
                    or not any(host == allowed or host.endswith("." + allowed) for allowed in ARTWORK_HOSTS)):
                continue
        except ValueError:
            continue
        width, height = thumbnail.get("width"), thumbnail.get("height")
        size = max(width, height) if (type(width) is int and type(height) is int
                                     and 0 < width <= 8192 and 0 < height <= 8192) else 8192
        candidates.append((abs(size - 120), size, url))
    return min(candidates)[2] if candidates else None


def artist_names(item):
    artists = item.get("artists") or item.get("author") or []
    if not isinstance(artists, list):
        return ""
    names = [clean(artist.get("name")) for artist in artists[:30] if isinstance(artist, dict)]
    return clean(", ".join(name for name in names if name))


def artist_links(item):
    """Retain at most five canonical artist references; never guess from a name."""
    values = item.get("artists") or item.get("author") or []
    result = []
    seen = set()
    for item in values[:30] if isinstance(values, list) else []:
        if not isinstance(item, dict):
            continue
        artist = item.get("id")
        name = clean(item.get("name"))
        if isinstance(artist, str) and re.fullmatch(r"UC[A-Za-z0-9_-]{22}", artist) and name and artist not in seen:
            result.append({"id": artist, "name": name})
            seen.add(artist)
            if len(result) == 5:
                break
    return result


def duration_seconds(item):
    value = item.get("duration_seconds")
    if isinstance(value, int) and not isinstance(value, bool) and 0 < value < 2**32:
        return value
    text = item.get("duration")
    if isinstance(text, str) and re.fullmatch(r"[0-9]{1,6}:[0-5][0-9](?::[0-5][0-9])?", text):
        parts = [int(part) for part in text.split(":")]
        seconds = 0
        for part in parts:
            seconds = seconds * 60 + part
        return seconds or None
    return None


def tracks(items, limit=MAX_TRACKS, exclude=(), deduplicate=False):
    result = []
    if not isinstance(items, list):
        return result
    seen = set(exclude)
    for item in items[:MAX_TRACKS]:
        if not isinstance(item, dict) or item.get("isAvailable") is False:
            continue
        video = identifier(item.get("videoId"), True)
        if not video or video in seen:
            continue
        if deduplicate:
            seen.add(video)
        album = item.get("album") or {}
        result.append({
            "title": clean(item.get("title")),
            "artist": artist_names(item),
            "artists": artist_links(item),
            "album": clean(album.get("name") if isinstance(album, dict) else ""),
            "source": "https://www.youtube.com/watch?v=" + video,
            "artwork_url": artwork_url(item),
            "duration_seconds": duration_seconds(item),
        })
        if len(result) >= limit:
            break
    return result


def playlist_entry(value):
    return value if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,256}", value) else None


def playlist_context(playlist, data):
    items = data.get("tracks", []) if isinstance(data, dict) else []
    items = items if isinstance(items, list) else []
    entries = [playlist_entry(item.get("setVideoId"))
               for item in items[:MAX_TRACKS] if isinstance(item, dict)
               and item.get("isAvailable") is not False and identifier(item.get("videoId"), True)]
    return {"id": playlist, "editable": data.get("owned") is True, "entries": entries}


def feedback(client, request):
    """Never replay a collection write, or serialize the provider's raw reply."""
    fields = {
        "rating": {"kind", "video_id"},
        "rate": {"kind", "video_id", "rating"},
        "playlistadd": {"kind", "video_id", "playlist_id"},
        "playlistremove": {"kind", "video_id", "playlist_id", "entry_id"},
        "history": {"kind", "video_id"},
    }
    if not isinstance(request, dict):
        return {"type": "error", "code": "feedback_invalid"}
    kind = request.get("kind")
    video = identifier(request.get("video_id"), True)
    if not isinstance(kind, str) or kind not in fields or set(request) != fields[kind] or not video:
        return {"type": "error", "code": "feedback_invalid"}
    playlist = identifier(request.get("playlist_id")) if kind.startswith("playlist") else None
    rating = request.get("rating") if kind == "rate" else None
    entry = playlist_entry(request.get("entry_id")) if kind == "playlistremove" else None
    if (kind.startswith("playlist") and not playlist
            or kind == "rate" and rating not in ("LIKE", "DISLIKE", "INDIFFERENT")
            or kind == "playlistremove" and not entry):
        return {"type": "error", "code": "feedback_invalid"}
    try:
        if kind == "rating":
            data = client.get_watch_playlist(videoId=video, limit=1)
            items = data.get("tracks", []) if isinstance(data, dict) else []
            for item in items[:50] if isinstance(items, list) else []:
                if isinstance(item, dict) and item.get("videoId") == video:
                    value = item.get("likeStatus")
                    rating = value if value in ("LIKE", "DISLIKE", "INDIFFERENT") else None
                    break
        elif kind == "rate":
            response = client.rate_song(video, rating)
            if not isinstance(response, dict) or "error" in response:
                raise ValueError("unconfirmed rating")
        elif kind in ("playlistadd", "playlistremove"):
            data = client.get_playlist(playlist, limit=MAX_TRACKS + 1 if entry else 1)
            if not isinstance(data, dict) or data.get("owned") is not True:
                return {"type": "error", "code": "feedback_readonly"}
            if entry:
                items = data.get("tracks", [])
                if not isinstance(items, list) or not any(
                        isinstance(item, dict) and item.get("videoId") == video
                        and item.get("setVideoId") == entry for item in items[:MAX_TRACKS]):
                    return {"type": "error", "code": "feedback_entry_changed"}
                response = client.remove_playlist_items(playlist, [{"videoId": video, "setVideoId": entry}])
            else:
                response = client.add_playlist_items(playlist, [video], duplicates=False)
            status = response.get("status") if isinstance(response, dict) else response
            if status != "STATUS_SUCCEEDED":
                raise ValueError("unconfirmed playlist edit")
        else:
            # Both calls must use this same authenticated client. Tracking data
            # stays in this process and can only target YouTube's playback endpoint.
            song = client.get_song(video)
            url = song.get("playbackTracking", {}).get("videostatsPlaybackUrl", {}).get("baseUrl")
            if not isinstance(url, str) or len(url) > 8192 or any(ord(c) < 32 for c in url):
                raise ValueError("invalid playback tracking")
            parsed = urlsplit(url)
            if (parsed.scheme != "https" or parsed.hostname not in
                    ("s.youtube.com", "music.youtube.com", "www.youtube.com", "youtube.com")
                    or parsed.username or parsed.password or parsed.port not in (None, 443)
                    or parsed.fragment or parsed.path != "/api/stats/playback"):
                raise ValueError("invalid playback destination")
            if getattr(client.add_history_item(song), "status_code", None) != 204:
                raise ValueError("unconfirmed history")
        return {"type": "feedback", "result": {"operation": kind, "video_id": video,
                "playlist_id": playlist, "rating": rating, "entry_id": entry}}
    except Exception:
        # A timeout might follow a completed write. Leave the result unconfirmed;
        # do not retry, disconnect the player, or expose response URLs/headers.
        return {"type": "error", "code": "feedback_unavailable" if kind == "rating" else "feedback_unconfirmed"}


def lyrics(client, video):
    """Read only this video's lyrics; never return credentials or raw errors."""
    empty = {"type": "lyrics", "lines": [], "synced": False, "source": ""}
    if not identifier(video, True):
        return empty
    try:
        watch = client.get_watch_playlist(videoId=video, limit=1)
        browse = watch.get("lyrics") if isinstance(watch, dict) else None
        if not isinstance(browse, str) or not re.fullmatch(r"MPLYt[A-Za-z0-9_=\-]{1,500}", browse):
            return empty
        data = client.get_lyrics(browse, timestamps=True)
        if not isinstance(data, dict):
            return empty
        synced = data.get("hasTimestamps") is True
        raw = data.get("lyrics")
        if synced and not isinstance(raw, list):
            return empty
        if not synced and not isinstance(raw, str):
            return empty
        if not synced and len(raw.encode("utf-8")) > 128_000:
            return empty
        result, total, previous = [], 0, -1
        for item in (raw if synced else raw.splitlines())[:1000]:
            if synced:
                text = item.get("text") if isinstance(item, dict) else getattr(item, "text", None)
                start = item.get("start_time") if isinstance(item, dict) else getattr(item, "start_time", None)
                if type(start) is not int or not 0 <= start <= 86_400_000 or start < previous:
                    return empty
                previous = start
                end = item.get("end_time") if isinstance(item, dict) else getattr(item, "end_time", None)
                if end is not None and (type(end) is not int or not start <= end <= 86_400_000):
                    return empty
            else:
                text, start, end = item, None, None
            if not isinstance(text, str):
                return empty
            text = clean(text.replace("\n", " "), 500)
            if not text.strip():
                # Retain timed empty lines: they clear the highlight for breaks.
                if not synced:
                    continue
                text = ""
            total += len(text.encode("utf-8"))
            if total > 128_000:
                return empty
            result.append({"text": text, "start_ms": start, "end_ms": end})
        return {"type": "lyrics", "lines": result, "synced": synced,
                "source": clean(data.get("source"), 120) if isinstance(data.get("source"), str) else ""}
    except Exception:
        return {"type": "error", "code": "lyrics_unavailable"}


def run(client, request):
    if request.get("action") == "lyrics":
        return lyrics(client, request.get("video_id"))
    action = request["action"]
    if action == "feedback":
        return feedback(client, request.get("request"))
    if action == "artists":
        video = identifier(request.get("video_id"), True)
        if not video:
            return {"type": "error", "code": "artist_unavailable"}
        data = client.get_watch_playlist(videoId=video, limit=1)
        items = data.get("tracks") if isinstance(data, dict) else None
        for item in items[:50] if isinstance(items, list) else []:
            if isinstance(item, dict) and item.get("videoId") == video:
                return {"type": "artists", "artists": artist_links(item)}
        return {"type": "artists", "artists": []}
    if action == "artist":
        artist = request.get("id")
        if not isinstance(artist, str) or not re.fullmatch(r"UC[A-Za-z0-9_-]{22}", artist):
            return {"type": "error", "code": "artist_unavailable"}
        data = client.get_artist(artist)
        if not isinstance(data, dict) or not clean(data.get("name")):
            return {"type": "error", "code": "artist_unavailable"}
        name = clean(data["name"])
        songs = data.get("songs")
        songs = songs if isinstance(songs, dict) else {}
        items = songs.get("results")
        items = items if isinstance(items, list) else []
        playlist = identifier(songs.get("browseId"))
        if playlist:
            # Top-song lists may initially contain only five results. Expansion
            # is bounded and optional: retain the returned top songs on failure.
            try:
                expanded = client.get_playlist(playlist, limit=51)
                if isinstance(expanded, dict) and isinstance(expanded.get("tracks"), list):
                    items = expanded["tracks"]
            except Exception:
                pass
        bounded = []
        for item in items[:51]:
            if isinstance(item, dict):
                item = dict(item)
                if not artist_links(item):
                    item["artists"] = [{"id": artist, "name": name}]
                bounded.append(item)
        return {"type": "artist", "name": name, "tracks": tracks(bounded, 50, (), True),
                "truncated": len(items) > 50}
    if action == "library":
        info = client.get_account_info()
        playlists = client.get_library_playlists(limit=201)
        return {"type": "library", "account": clean(info.get("accountName") or info.get("name") or "YouTube Music"),
                "playlists": [{"id": identifier(p.get("playlistId")), "title": clean(p.get("title")),
                               "subtitle": clean(str(p.get("count") or "") + " tracks"),
                               "artwork_url": artwork_url(p), "owned": p.get("owned") is True}
                              for p in playlists[:200] if isinstance(p, dict) and identifier(p.get("playlistId"))],
                "truncated": len(playlists) > 200}
    if action == "supermix":
        # Resolve this account's dynamic mix on demand, independently of Home's
        # small display pool. Never substitute another mix or a public playlist.
        try:
            shelves = client.get_home(limit=MAX_SUPERMIX_SHELVES)
            for shelf in shelves[:MAX_SUPERMIX_SHELVES] if isinstance(shelves, list) else []:
                items = shelf.get("contents") if isinstance(shelf, dict) else None
                for item in items[:MAX_HOME_ITEMS_PER_SHELF] if isinstance(items, list) else []:
                    if not isinstance(item, dict) or item.get("isAvailable") is False:
                        continue
                    playlist = identifier(item.get("playlistId"))
                    title = "".join(clean(item.get("title")).casefold().split())
                    if not playlist.startswith("RDTMAK") or title not in ("mysupermix", "yoursupermix", "supermix"):
                        continue
                    data = client.get_watch_playlist(playlistId=playlist, limit=50)
                    songs = data.get("tracks") if isinstance(data, dict) else None
                    if not isinstance(songs, list):
                        return {"type": "error", "code": "supermix_unavailable"}
                    selected = tracks(songs, MAX_TRACKS, (), True)
                    if not selected:
                        return {"type": "error", "code": "supermix_unavailable"}
                    return {"type": "tracks", "tracks": selected, "truncated": len(songs) > MAX_TRACKS}
            return {"type": "error", "code": "supermix_unavailable"}
        except Exception as error:
            return verified_request_failure(error, client)
    if action == "radio":
        video = identifier(request.get("video_id"), True)
        empty = {"type": "tracks", "tracks": [], "truncated": False}
        if not video:
            return empty
        try:
            # The API's limit is a minimum; normalize and cap the actual output.
            # This read does not call any history, rating, or library write API.
            data = client.get_watch_playlist(videoId=video, limit=MAX_RADIO_TRACKS, radio=True)
            items = data.get("tracks", []) if isinstance(data, dict) else []
            if not isinstance(items, list):
                return empty
            return {"type": "tracks", "tracks": tracks(items, MAX_RADIO_TRACKS, (video,), True),
                    "truncated": len(items) > MAX_RADIO_TRACKS}
        except Exception as error:
            return verified_request_failure(error, client)
    if action in ("playlist", "liked"):
        if action == "playlist":
            playlist = identifier(request.get("id"))
            if not playlist:
                raise ValueError("invalid identifier")
            data = client.get_playlist(playlist, limit=MAX_TRACKS + 1)
        else:
            data = client.get_liked_songs(limit=MAX_TRACKS + 1)
        items = data.get("tracks", [])
        result = {"type": "tracks", "tracks": tracks(items), "truncated": len(items) > MAX_TRACKS}
        if action == "playlist":
            result["playlist"] = playlist_context(playlist, data)
        return result
    if action == "home":
        # Keep the leading Home rows in provider order. Fetching distant shelves
        # for novelty diluted the earlier, more relevant selection.
        # The API may overshoot its row limit; cap our output too.
        shelves = client.get_home(limit=MAX_HOME_SHELVES)
        cards = []
        seen = set()
        # ytmusicapi includes description shelves with string contents and None
        # entries for unsupported mixed content. Keep the playable shelf rows.
        if not isinstance(shelves, list):
            return {"type": "home", "cards": cards}
        shelves = [shelf for shelf in shelves if isinstance(shelf, dict)
                   and isinstance(shelf.get("contents"), list)]
        for shelf in shelves[:MAX_HOME_SHELVES]:
            for item in shelf["contents"][:MAX_HOME_ITEMS_PER_SHELF]:
                if not isinstance(item, dict) or item.get("isAvailable") is False:
                    continue
                kind = clean(item.get("type") or "unknown", 30).lower()
                video = identifier(item.get("videoId"), True)
                playlist = identifier(item.get("playlistId"))
                if video:
                    kind, item_id = "song", video
                elif playlist and kind not in ("album", "artist"):
                    kind, item_id = "playlist", playlist
                else:
                    item_id = identifier(item.get("browseId"))
                    if item_id.startswith("MPRE"):
                        kind = "album"
                    elif item_id.startswith("UC"):
                        kind = "artist"
                key = (kind, item_id)
                if not item_id or key in seen:
                    continue
                seen.add(key)
                cards.append({"id": item_id, "title": clean(item.get("title")),
                              "subtitle": artist_names(item), "artists": artist_links(item),
                              "kind": kind, "shelf": clean(shelf.get("title")),
                              "artwork_url": artwork_url(item),
                              "duration_seconds": duration_seconds(item)})
                if len(cards) >= MAX_HOME_CARDS:
                    return {"type": "home", "cards": cards}
        return {"type": "home", "cards": cards}
    raise ValueError("unsupported action")


def main():
    try:
        raw = sys.stdin.buffer.read(MAX_INPUT + 1)
        if len(raw) > MAX_INPUT:
            raise ValueError("input limit")
        request = json.loads(raw)
        if request.get("action") == "verify_import":
            headers = session_headers(request.get("headers", {}))
            if headers and validate_session(headers):
                result = {"type": "session", "headers": headers}
            else:
                result = {"type": "error", "code": "session"}
            print(json.dumps(result, ensure_ascii=True))
            return
        if request.get("action") == "switchaccount":
            result = switch_account(request.get("headers"), request.get("index"))
            print(json.dumps(result, ensure_ascii=True))
            return
        if request.get("action") == "connect_existing":
            result = connect_existing(request["database"], request.get("index", 0))
            print(json.dumps(result, ensure_ascii=True))
            return
        if request.get("action") == "connect":
            result = connect(request["directory"])
            print(json.dumps(result, ensure_ascii=True))
            return
        client, session = create_client(request.pop("headers"))
        try:
            try:
                result = run(client, request)
            except Exception as error:
                result = verified_request_failure(error, client)
            updated = getattr(session, "_tide_refreshed_headers", None)
            if updated and (result.get("type") != "error" or request.get("action") in ("feedback", "lyrics")):
                result["refreshed_headers"] = updated
        finally:
            session.close()
        print(json.dumps(result, ensure_ascii=True))
    except ImportError:
        print('{"type":"error","code":"runtime"}')
    except Exception as error:
        # Exceptions can include HTTP headers or request data: never serialize them.
        print(json.dumps(request_failure(error)))


if __name__ == "__main__":
    main()

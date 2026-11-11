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
import unicodedata
import warnings
from itertools import islice
from urllib.parse import urlsplit

MAX_INPUT = 65536
MAX_TRACKS = 2000
MAX_RADIO_TRACKS = 25
# Autoplay reads further into the personalized Up next queue so songs already
# queued or played can be skipped without running out.
MAX_AUTOPLAY_FETCH = 150
MAX_AUTOPLAY_EXCLUDE = 400
# Home keeps YouTube Music's shelf structure (Quick picks, Mixed for you, New
# releases, ...). The app gives every shelf its own slot budget and rotates
# within it on refresh, so more shelves mean variety without diluting the top.
MAX_HOME_SHELVES = 9
MAX_HOME_ITEMS_PER_SHELF = 30
MAX_HOME_CARDS = MAX_HOME_SHELVES * MAX_HOME_ITEMS_PER_SHELF
MAX_SUPERMIX_SHELVES = 20
MAX_LIBRARY_CARDS = 200
MAX_SEARCH_CARDS = 40
MAX_EXPLORE_CATEGORIES = 128
ARTWORK_SIDE = 160  # Match the bounded native thumbnail cache, not full-size artwork.
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
    try:
        return verify_candidate(headers)
    except ImportError:
        return {"type": "error", "code": "runtime"}
    except Exception as error:
        return request_failure(error)


def switch_account(headers, index):
    if type(index) is not int or not 0 <= index <= 9 or not isinstance(headers, dict):
        return {"type": "error", "code": "session"}
    try:
        # Normalize a separate candidate. Failed verification must leave the
        # caller's current session unchanged and must not return new headers.
        candidate_source = {name: value for name, value in headers.items()
                            if name.lower() != "x-goog-pageid"}
        candidate = session_headers({**candidate_source, "x-goog-authuser": str(index)})
        return verify_candidate(candidate)
    except ImportError:
        return {"type": "error", "code": "runtime"}
    except Exception as error:
        return request_failure(error)


def create_client(auth):
    from ytmusicapi import YTMusic
    from ytmusicapi.helpers import initialize_headers
    import requests
    from functools import partial
    session = requests.Session()
    session._tide_request_timeout = 20
    transport = session.request

    def contained_request(*args, **kwargs):
        # Session.get supplies allow_redirects=True itself, which would override
        # a partial's default. Enforce the boundary at the final request call.
        kwargs["allow_redirects"] = False
        kwargs["timeout"] = session._tide_request_timeout
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


def verify_candidate(headers):
    if not headers:
        return {"type": "error", "code": "session"}
    if validate_session(headers):
        return {"type": "session", "headers": headers}
    # An empty or unfamiliar identity response cannot verify the candidate.
    # Explicit denials are classified by request_failure at the call boundary.
    return {"type": "error", "code": "request"}


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


def artwork_url(item, side=ARTWORK_SIDE, minimum=0):
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
        if not minimum or minimum <= size <= 1000:
            candidates.append((abs(size - side), size, url))
    return min(candidates)[2] if candidates else None


def featured_artwork_url(item, small):
    large = artwork_url(item, side=320, minimum=240)
    if large or not small:
        return large
    # YouTube Music often returns only 60px and 120px variants. Its public
    # Google artwork URLs encode the dimensions in a final path suffix.
    parsed = urlsplit(small)
    host = parsed.hostname or ""
    if parsed.query or parsed.fragment or not any(
            host == domain or host.endswith("." + domain)
            for domain in ("googleusercontent.com", "ggpht.com")):
        return None
    larger, count = re.subn(r"=w[0-9]{1,4}-h[0-9]{1,4}(-l[0-9]{1,3}-rj)$",
                           lambda match: "=w320-h320" + match.group(1), small)
    return larger if count and larger != small else None


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


def autoplay_tracks(client, video, exclude):
    """Continue like YouTube Music's own autoplay.

    Without radio parameters the watch endpoint returns the signed-in
    listener's personalized Up next queue (the automix YouTube Music plays
    after a song). Songs already queued or played are skipped; if nothing new
    remains, fall back to the song radio so playback can continue.
    """
    limit = min(MAX_AUTOPLAY_FETCH, MAX_RADIO_TRACKS + len(exclude))
    data = client.get_watch_playlist(videoId=video, limit=limit)
    items = data.get("tracks", []) if isinstance(data, dict) else []
    selected = radio_tracks(items if isinstance(items, list) else [], video, exclude)
    if not selected:
        data = client.get_watch_playlist(videoId=video, limit=MAX_RADIO_TRACKS, radio=True)
        items = data.get("tracks", []) if isinstance(data, dict) else []
        selected = radio_tracks(items if isinstance(items, list) else [], video, exclude)
    return selected


def radio_tracks(items, seed, exclude=()):
    """Keep the service's ranking and prefer an explicitly identified album track."""
    excluded = {seed, *exclude}
    candidates = []
    for item in items[:MAX_TRACKS]:
        if not isinstance(item, dict):
            continue
        video = identifier(item.get("videoId"), True)
        if not video:
            continue
        counterpart = item.get("counterpart")
        counterpart = counterpart if isinstance(counterpart, dict) else {}
        alternate = identifier(counterpart.get("videoId"), True)
        if (seed in (video, alternate) or item.get("likeStatus") == "DISLIKE"
                or counterpart.get("likeStatus") == "DISLIKE"):
            excluded.update((video, alternate))
        if item.get("isAvailable") is False or item.get("videoType") == "MUSIC_VIDEO_TYPE_PODCAST_EPISODE":
            continue
        if (alternate and counterpart.get("videoType") == "MUSIC_VIDEO_TYPE_ATV"
                and counterpart.get("isAvailable") is not False
                and item.get("videoType") != "MUSIC_VIDEO_TYPE_ATV"):
            chosen = {**counterpart}
            if not artwork_url(chosen):
                chosen["thumbnail"] = item.get("thumbnail")
                chosen["thumbnails"] = item.get("thumbnails")
        else:
            chosen = {**item}
        chosen["duration"] = chosen.get("duration") or chosen.get("length")
        candidates.append((video, chosen))
    return tracks([item for original, item in candidates if original not in excluded],
                  MAX_RADIO_TRACKS, excluded, True)


def browse_cards(items, kind, shelf):
    """Normalize read-only library/history data; never return opaque API tokens."""
    cards = []
    seen = set()
    for item in items[:MAX_LIBRARY_CARDS + 1] if isinstance(items, list) else []:
        if not isinstance(item, dict) or item.get("isAvailable") is False:
            continue
        item_id = identifier(item.get("videoId"), True) if kind == "song" else identifier(item.get("browseId"))
        if kind == "artist" and not re.fullmatch(r"UC[A-Za-z0-9_-]{22}", item_id):
            continue
        if kind == "album" and not re.fullmatch(r"MPRE[A-Za-z0-9_-]{4,156}", item_id):
            continue
        title = clean(item.get("artist") if kind == "artist" else item.get("title"))
        if not item_id or not title or (kind != "song" and item_id in seen):
            continue
        seen.add(item_id)
        cards.append({"id": item_id, "title": title,
                      "subtitle": artist_names(item), "artists": artist_links(item),
                      "kind": kind, "shelf": (clean(item.get("played")) or shelf) if kind == "song" else shelf,
                      "artwork_url": artwork_url(item),
                      "duration_seconds": duration_seconds(item) if kind == "song" else None})
        if len(cards) == MAX_LIBRARY_CARDS:
            break
    return cards


def playlist_entry(value):
    return value if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,256}", value) else None


def playlist_count_label(count):
    if isinstance(count, str):
        count = count.strip()
        if len(count) <= 15 and re.fullmatch(r"[0-9]+|[0-9]{1,3}(?:,[0-9]{3})+", count):
            count = int(count.replace(",", ""))
    if type(count) is int and 0 <= count < 10**12:
        return f"{count:,} track" if count == 1 else f"{count:,} tracks"
    return ""


def playlist_context(playlist, data):
    items = data.get("tracks", []) if isinstance(data, dict) else []
    items = items if isinstance(items, list) else []
    entries = [playlist_entry(item.get("setVideoId"))
               for item in items[:MAX_TRACKS] if isinstance(item, dict)
               and item.get("isAvailable") is not False and identifier(item.get("videoId"), True)]
    return {"id": playlist, "editable": data.get("owned") is True, "entries": entries}


def valid_playlist_title(title):
    return (isinstance(title, str) and bool(title.strip()) and len(title) <= 100
            and not any(ord(c) < 32 or 127 <= ord(c) <= 159 or c in "<>" for c in title))


def manage_playlist(client, request, kind):
    fields = {"kind"}
    if kind != "playlistcreate":
        fields.add("playlist_id")
    if kind != "playlistdelete":
        fields.add("title")
    title = request.get("title")
    playlist = identifier(request.get("playlist_id"))
    if (set(request) != fields or ("title" in fields and not valid_playlist_title(title))
            or ("playlist_id" in fields and not (playlist.startswith("PL") and len(playlist) > 2))):
        return {"type": "error", "code": "feedback_invalid"}
    try:
        if kind == "playlistcreate":
            playlist = identifier(client.create_playlist(title.strip(), "", privacy_status="PRIVATE"))
            confirmed = playlist.startswith("PL") and len(playlist) > 2
        else:
            data = client.get_playlist(playlist, limit=1)
            if not isinstance(data, dict) or data.get("owned") is not True:
                return {"type": "error", "code": "feedback_readonly"}
            response = (client.edit_playlist(playlist, title=title.strip()) if kind == "playlistrename"
                        else client.delete_playlist(playlist))
            status = response.get("status") if isinstance(response, dict) else response
            confirmed = status == "STATUS_SUCCEEDED"
        if confirmed:
            return {"type": "feedback", "result": {"operation": kind, "video_id": "",
                    "playlist_id": playlist, "rating": None, "entry_id": None}}
    except Exception:
        # A timed-out write may have taken effect. Never retry it automatically.
        pass
    return {"type": "error", "code": "feedback_unconfirmed"}


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
    if kind in ("playlistcreate", "playlistrename", "playlistdelete"):
        return manage_playlist(client, request, kind)
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


def lrc_lines(raw, duration):
    """Parse bounded timestamp text as data, never markup or executable input."""
    if not isinstance(raw, str) or len(raw.encode("utf-8")) > 128_000:
        return None
    if re.search(r"\[offset:[+-]?[1-9]", raw, re.I):
        return None
    stamp = re.compile(r"\[(\d{1,3}):([0-5]\d)(?:[.:](\d{1,3}))?\]")
    lines = []
    for row in raw.splitlines()[:2000]:
        times, position = [], 0
        while match := stamp.match(row, position):
            ms = (int(match[1]) * 60 + int(match[2])) * 1000 + int((match[3] or "0").ljust(3, "0"))
            if ms > (duration + 5) * 1000:
                return None
            times.append(ms)
            position = match.end()
        text = clean(row[position:], 500).strip()
        lines.extend({"text": text, "start_ms": ms, "end_ms": None} for ms in times)
        if len(lines) > 1000:
            return None
    lines.sort(key=lambda line: line["start_ms"])
    if sum(len(line["text"].encode("utf-8")) for line in lines) > 128_000:
        return None
    return lines or None


def word_timed_lines(raw, duration):
    """Read bounded Lyricsfile data without YAML tags, anchors or aliases."""
    if not isinstance(raw, str) or len(raw.encode("utf-8")) > 128_000:
        return None
    try:
        from ruamel.yaml import YAML
        from ruamel.yaml.tokens import AliasToken, AnchorToken, TagToken
        parser = YAML(typ="safe", pure=True)
        parser.max_depth = 12
        if any(isinstance(token, (AliasToken, AnchorToken, TagToken)) for token in parser.scan(raw)):
            return None
        document = parser.load(raw)
    except Exception:
        return None

    rows = document.get("lines") if isinstance(document, dict) else None
    if not isinstance(rows, list) or not 1 <= len(rows) <= 1000:
        return None
    result, previous = [], -1
    limit = (duration + 5) * 1000
    for row in rows:
        if not isinstance(row, dict):
            return None
        text, start, end = row.get("text"), row.get("start_ms"), row.get("end_ms")
        if (not isinstance(text, str) or len(text) > 500 or clean(text, 500) != text
                or type(start) is not int or not max(previous, 0) <= start <= limit
                or end is not None and (type(end) is not int or not start <= end <= limit)):
            return None
        previous = start
        words = row.get("words", [])
        if not isinstance(words, list) or len(words) > 100:
            return None
        ends, pieces, last = [], [], start
        for word in words:
            if not isinstance(word, dict):
                return None
            piece, at = word.get("text"), word.get("start_ms")
            if not isinstance(piece, str) or not piece or type(at) is not int or not last <= at <= (end if end is not None else limit):
                return None
            pieces.append(piece)
            ends.append(at)
            last = at
        line = {"text": text, "start_ms": start, "end_ms": end}
        # The desktop highlight clips a left-to-right text run. Mixed-direction
        # lyrics retain whole-line highlighting.
        if (ends and "".join(pieces) == text
                and not any(unicodedata.bidirectional(char) in ("R", "AL", "AN") for char in text)):
            boundary = 0
            line["words"] = []
            for piece, at in zip(pieces, ends):
                boundary += len(piece.encode("utf-8"))
                line["words"].append({"start_ms": at, "end_byte": boundary})
            has_words = True
        result.append(line)
    return result


def lrclib_lyrics(watch, video):
    """Public metadata only: NEVER use the authenticated YouTube session here."""
    from urllib.request import Request, urlopen
    from urllib.error import HTTPError
    from urllib.parse import urlencode
    items = watch.get("tracks") if isinstance(watch, dict) else None
    track = next((item for item in items[:50] if isinstance(item, dict) and item.get("videoId") == video), None) if isinstance(items, list) else None
    if not track:
        return None
    title, artist = clean(track.get("title")), artist_names(track)
    duration = duration_seconds({**track, "duration": track.get("duration") or track.get("length")})
    if not title or not artist or not duration or not 1 <= duration <= 3600:
        return None
    def lookup(endpoint, params):
        request = Request("https://lrclib.net/api/" + endpoint + "?" + urlencode(params),
                          headers={"User-Agent": "Tide/0.4.10 (https://github.com/yotam1001/tide-music)"})
        # A separate standard-library request carries no Google cookies/headers.
        with urlopen(request, timeout=3) as response:
            raw = response.read(256_001)
        return json.loads(raw) if len(raw) <= 256_000 else None

    def normalize(value):
        return "".join(c for c in value.casefold() if c.isalnum()) if isinstance(value, str) else ""

    params = {"track_name": title, "artist_name": artist}
    try:
        try:
            record = lookup("get", {**params, "duration": duration})
        except HTTPError as error:
            if error.code != 404:
                raise
            record = None

        def matched_lines(candidate):
            if not isinstance(candidate, dict):
                return None
            if (normalize(candidate.get("trackName")) != normalize(title)
                    or normalize(candidate.get("artistName")) != normalize(artist)):
                return None
            recorded_duration = candidate.get("duration")
            if type(recorded_duration) not in (int, float) or not abs(recorded_duration - duration) <= 2:
                return None
            lines = (word_timed_lines(candidate.get("lyricsfile"), duration)
                     if candidate.get("hasWordSync") is True else None)
            lines = lines or lrc_lines(candidate.get("syncedLyrics"), duration)
            return lines if lines and any(line["text"] for line in lines) else None

        lines = matched_lines(record)
        if not lines:
            candidates = lookup("search", params)
            if isinstance(candidates, list):
                for candidate in candidates[:20]:
                    lines = matched_lines(candidate)
                    if lines:
                        break
        if lines:
            return {"type": "lyrics", "lines": lines, "synced": True, "source": "Source: LRCLIB", "retry_after_seconds": 0}
    except HTTPError as error:
        if error.code in (429, 503):
            value = error.headers.get("Retry-After", "60") if error.headers else "60"
            try:
                seconds = int(value)
            except (ValueError, TypeError):
                from email.utils import parsedate_to_datetime
                try:
                    seconds = int(parsedate_to_datetime(value).timestamp() - time.time())
                except (ValueError, TypeError, OverflowError):
                    seconds = 60
            return {"type": "lyrics", "lines": [], "synced": False, "source": "", "retry_after_seconds": max(1, min(86400, seconds))}
    except Exception:
        pass
    return None


def lyrics(client, video, allow_external=True):
    """Read only this video's lyrics; never return credentials or raw errors."""
    empty = {"type": "lyrics", "lines": [], "synced": False, "source": ""}
    if not identifier(video, True):
        return empty
    try:
        watch = client.get_watch_playlist(videoId=video, limit=1)
        browse = watch.get("lyrics") if isinstance(watch, dict) else None
        data = None
        failed = False
        if isinstance(browse, str) and re.fullmatch(r"MPLYt[A-Za-z0-9_=\-]{1,500}", browse):
            try:
                data = client.get_lyrics(browse, timestamps=True)
            except Exception:
                try:
                    data = client.get_lyrics(browse, timestamps=False)
                except Exception:
                    failed = True
        native_synced = isinstance(data, dict) and data.get("hasTimestamps") is True
        fallback = lrclib_lyrics(watch, video) if allow_external else None
        if fallback and fallback["lines"] and (
                not native_synced or any(line.get("words") for line in fallback["lines"])):
            return fallback
        if not isinstance(data, dict):
            return fallback or ({"type": "error", "code": "lyrics_unavailable"} if failed else empty)
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
                "source": clean(data.get("source"), 120) if isinstance(data.get("source"), str) else "",
                "retry_after_seconds": (fallback or {}).get("retry_after_seconds", 0)}
    except Exception:
        return {"type": "error", "code": "lyrics_unavailable"}


def mood_params(value):
    """Public opaque browse parameters, never a URL or filesystem destination."""
    return isinstance(value, str) and 1 <= len(value) <= 2048 and all(33 <= ord(c) < 127 for c in value)


def run(client, request):
    if request.get("action") == "lyrics":
        return lyrics(client, request.get("video_id"), request.get("allow_external", True) is True)
    action = request["action"]
    if action == "explore":
        explore_error = None
        try:
            data = client.get_explore()
        except Exception as error:
            data, explore_error = {}, error
        data = data if isinstance(data, dict) else {}
        cards = []
        truncated = False
        for key, kind, title in [("new_releases", "album", "New releases"),
                                 ("top_songs", "song", "Top songs"),
                                 ("trending", "song", "Trending")]:
            items = data.get(key, [])
            if isinstance(items, dict):
                items = items.get("items", [])
            items = items if isinstance(items, list) else []
            truncated |= len(items) > MAX_SEARCH_CARDS
            music = [item for item in items[:MAX_SEARCH_CARDS]
                     if isinstance(item, dict) and not item.get("podcast")
                     and item.get("resultType") not in ("podcast", "episode")
                     and item.get("videoType") != "MUSIC_VIDEO_TYPE_PODCAST_EPISODE"]
            cards.extend(browse_cards(music, kind, title))
        try:
            category_data = client.get_mood_categories()
            categories_available = isinstance(category_data, dict)
        except Exception:
            # A category/parser failure must not discard otherwise valid charts.
            category_data, categories_available = {}, False
        if not categories_available or not category_data:
            # The main Explore response can carry usable category links even
            # when the dedicated catalog is temporarily unavailable.
            category_data = {"Moods & genres": data.get("moods_and_genres")}
        categories, seen = [], set()
        for group, items in islice(category_data.items(), 8):
            if not isinstance(group, str) or not isinstance(items, list):
                continue
            for item in items[:MAX_EXPLORE_CATEGORIES + 1]:
                if not isinstance(item, dict) or not mood_params(item.get("params")):
                    continue
                title, params = clean(item.get("title"), 80), item["params"]
                if not title or params in seen:
                    continue
                seen.add(params)
                if len(categories) == MAX_EXPLORE_CATEGORIES:
                    truncated = True
                    break
                categories.append({"title": title, "group": clean(group, 80), "params": params})
        if explore_error is not None and not categories:
            # With neither source available, preserve the original failure
            # so the normal verification and retry handling can classify it.
            raise explore_error
        return {"type": "explore", "cards": cards, "categories": categories,
                "categories_available": categories_available or bool(categories), "truncated": truncated}
    if action == "mood":
        params = request.get("params")
        if not mood_params(params):
            return {"type": "error", "code": "explore_unavailable"}
        items = client.get_mood_playlists(params)
        normalized = []
        for item in items[:MAX_LIBRARY_CARDS] if isinstance(items, list) else []:
            if not isinstance(item, dict):
                continue
            item = dict(item)
            identity = item.get("playlistId") or item.get("browseId")
            item["browseId"] = identity.removeprefix("VL") if isinstance(identity, str) else ""
            if isinstance(item.get("author"), dict):
                item["author"] = [item["author"]]
            normalized.append(item)
        return {"type": "mood", "cards": browse_cards(normalized, "playlist", ""),
                "truncated": isinstance(items, list) and len(items) > MAX_LIBRARY_CARDS}
    if action == "feedback":
        return feedback(client, request.get("request"))
    if action == "suggestions":
        query = request.get("query")
        if (not isinstance(query, str) or not 2 <= len(query.strip()) <= 120
                or any(not char.isprintable() for char in query)):
            return {"type": "suggestions", "items": []}
        items = client.get_search_suggestions(query.strip())
        suggestions = dict.fromkeys(clean(item, 120).strip()
                                    for item in (items[:20] if isinstance(items, list) else []) if isinstance(item, str))
        return {"type": "suggestions", "items": [item for item in suggestions if item][:5]}
    if action == "search":
        kind, query = request.get("kind"), request.get("query")
        kinds = {"songs": "song", "albums": "album", "artists": "artist", "playlists": "playlist"}
        if (not isinstance(kind, str) or kind not in kinds or not isinstance(query, str) or not query.strip()
                or len(query.encode("utf-8")) > 512 or any(ord(c) < 32 for c in query)):
            return {"type": "error", "code": "search_unavailable"}
        items = client.search(query.strip(), filter=kind, limit=MAX_SEARCH_CARDS)
        normalized = []
        for item in items[:MAX_SEARCH_CARDS] if isinstance(items, list) else []:
            if not isinstance(item, dict) or item.get("resultType") != kinds[kind]:
                continue
            item = dict(item)
            if kind == "playlists" and isinstance(item.get("browseId"), str):
                item["browseId"] = item["browseId"].removeprefix("VL")
            if not item.get("artists"):
                author = item.get("artist") if kind == "albums" else item.get("author")
                if isinstance(author, str):
                    item["artists"] = [{"name": author}]
            normalized.append(item)
        return {"type": "search", "kind": kind,
                "cards": browse_cards(normalized, kinds[kind], "Search results")[:MAX_SEARCH_CARDS]}
    if action in ("libraryalbums", "libraryartists", "history"):
        if action == "libraryalbums":
            items = client.get_library_albums(limit=MAX_LIBRARY_CARDS + 1)
            kind, shelf = "album", "Saved albums"
        elif action == "libraryartists":
            items = client.get_library_artists(limit=MAX_LIBRARY_CARDS + 1)
            kind, shelf = "artist", "Your artists"
        else:
            # This reads account history; it never records or removes a listen.
            items = client.get_history()
            kind, shelf = "song", "Listening history"
        return {"type": "browse", "section": {"libraryalbums": "albums", "libraryartists": "artists", "history": "history"}[action],
                "cards": browse_cards(items, kind, shelf),
                "truncated": isinstance(items, list) and len(items) > MAX_LIBRARY_CARDS}
    if action == "album":
        album = identifier(request.get("id"))
        if not re.fullmatch(r"MPRE[A-Za-z0-9_-]{4,156}", album):
            return {"type": "error", "code": "album_unavailable"}
        data = client.get_album(album)
        if not isinstance(data, dict) or not clean(data.get("title")):
            return {"type": "error", "code": "album_unavailable"}
        items = data.get("tracks")
        items = items if isinstance(items, list) else []
        normalized = []
        for item in items[:MAX_TRACKS]:
            if not isinstance(item, dict):
                continue
            item = dict(item)
            item["album"] = {"name": clean(data["title"])}
            if not artist_names(item):
                item["artists"] = data.get("artists")
            if not item.get("thumbnails"):
                item["thumbnails"] = data.get("thumbnails")
            normalized.append(item)
        return {"type": "tracks", "tracks": tracks(normalized), "truncated": len(items) > MAX_TRACKS}
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
                expanded_tracks = expanded.get("tracks") if isinstance(expanded, dict) else None
                if isinstance(expanded_tracks, list) and tracks(expanded_tracks[:51], limit=1):
                    items = expanded_tracks
            except Exception:
                pass
        bounded = []
        for item in items[:51]:
            if isinstance(item, dict):
                item = dict(item)
                if not artist_links(item):
                    item["artists"] = [{"id": artist, "name": name}]
                bounded.append(item)
        releases = []
        for key, label in (("albums", "Albums"), ("singles", "Singles & EPs")):
            section = data.get(key)
            if isinstance(section, dict):
                releases.extend(browse_cards(section.get("results"), "album", label)[:8])
        return {"type": "artist", "name": name, "tracks": tracks(bounded, 50, (), True),
                "truncated": len(items) > 50, "artwork_url": artwork_url(data, side=320),
                "releases": releases}
    if action == "library":
        info = client.get_account_info()
        playlists = client.get_library_playlists(limit=201)
        return {"type": "library", "account": clean(info.get("accountName") or info.get("name") or "YouTube Music"),
                "playlists": [{"id": identifier(p.get("playlistId")), "title": clean(p.get("title")),
                               "subtitle": playlist_count_label(p.get("count")),
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
                        continue
                    selected = tracks(songs, MAX_TRACKS, (), True)
                    if not selected:
                        continue
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
            return {"type": "tracks", "tracks": radio_tracks(items, video),
                    "truncated": len(items) > MAX_RADIO_TRACKS}
        except Exception as error:
            return verified_request_failure(error, client)
    if action == "autoplay":
        video = identifier(request.get("video_id"), True)
        empty = {"type": "tracks", "tracks": [], "truncated": False}
        if not video:
            return empty
        raw = request.get("exclude")
        exclude = {identifier(item, True) for item in (raw if isinstance(raw, list) else [])[:MAX_AUTOPLAY_EXCLUDE]
                   if isinstance(item, str)}
        exclude.discard("")
        try:
            return {"type": "tracks", "tracks": autoplay_tracks(client, video, exclude), "truncated": False}
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
        # Keep every shelf in provider order; the app picks from each one.
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
                thumbnail = artwork_url(item)
                cards.append({"id": item_id, "title": clean(item.get("title")),
                              "subtitle": artist_names(item), "artists": artist_links(item),
                              "kind": kind, "shelf": clean(shelf.get("title")),
                              "artwork_url": thumbnail,
                              "featured_artwork_url": featured_artwork_url(item, thumbnail),
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
            result = verify_candidate(headers)
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
        if request.get("action") == "suggestions":
            session._tide_request_timeout = 3
        if request.get("action") == "lyrics":
            # Both reads can time out and still return refreshed cookies before
            # the owning provider's 25-second overall deadline.
            session._tide_request_timeout = 5
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

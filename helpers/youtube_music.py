"""Read-only YouTube Music bridge. Credentials arrive on stdin, never argv."""
import json
import re
import sys
import warnings

MAX_INPUT = 65536
MAX_TRACKS = 2000
warnings.filterwarnings("ignore")


def clean(value, limit=300):
    return "".join(c for c in str(value or "") if c.isprintable())[:limit]


def identifier(value, video=False):
    value = str(value or "")
    pattern = r"[A-Za-z0-9_-]{11}" if video else r"[A-Za-z0-9_-]{1,160}"
    return value if re.fullmatch(pattern, value) else ""


def tracks(items):
    result = []
    for item in items[:MAX_TRACKS]:
        video = identifier(item.get("videoId"), True)
        if not video:
            continue
        album = item.get("album") or {}
        result.append({
            "title": clean(item.get("title")),
            "artist": clean(", ".join(a.get("name", "") for a in item.get("artists", []) or [])),
            "album": clean(album.get("name") if isinstance(album, dict) else ""),
            "source": "https://www.youtube.com/watch?v=" + video,
        })
    return result


def run(client, request):
    action = request["action"]
    if action == "library":
        info = client.get_account_info()
        playlists = client.get_library_playlists(limit=201)
        return {"type": "library", "account": clean(info.get("accountName") or info.get("name") or "YouTube Music"),
                "playlists": [{"id": identifier(p.get("playlistId")), "title": clean(p.get("title")),
                               "subtitle": clean(str(p.get("count") or "") + " tracks")}
                              for p in playlists[:200] if identifier(p.get("playlistId"))],
                "truncated": len(playlists) > 200}
    if action in ("playlist", "liked"):
        if action == "playlist":
            playlist = identifier(request.get("id"))
            if not playlist:
                raise ValueError("invalid identifier")
            data = client.get_playlist(playlist, limit=MAX_TRACKS + 1)
        else:
            data = client.get_liked_songs(limit=MAX_TRACKS + 1)
        items = data.get("tracks", [])
        return {"type": "tracks", "tracks": tracks(items), "truncated": len(items) > MAX_TRACKS}
    if action == "home":
        shelves = client.get_home(limit=3)
        cards = []
        for shelf in shelves[:3]:
            for item in shelf.get("contents", [])[:40]:
                kind = clean(item.get("type") or "unknown", 30)
                video = identifier(item.get("videoId"), True)
                playlist = identifier(item.get("playlistId"))
                if video:
                    kind, item_id = "song", video
                elif playlist and kind not in ("album", "artist"):
                    kind, item_id = "playlist", playlist
                else:
                    item_id = identifier(item.get("browseId"))
                cards.append({"id": item_id, "title": clean(item.get("title")),
                              "subtitle": clean(", ".join(a.get("name", "") for a in item.get("artists", []) or [])),
                              "kind": kind, "shelf": clean(shelf.get("title"))})
        return {"type": "home", "cards": cards}
    raise ValueError("unsupported action")


def main():
    try:
        raw = sys.stdin.buffer.read(MAX_INPUT + 1)
        if len(raw) > MAX_INPUT:
            raise ValueError("input limit")
        request = json.loads(raw)
        from ytmusicapi import YTMusic
        from ytmusicapi.helpers import initialize_headers
        import requests
        from functools import partial
        session = requests.Session()
        session.request = partial(session.request, timeout=20)
        headers = initialize_headers()
        headers.update(request.pop("headers"))
        client = YTMusic(auth=headers, user=headers.get("x-goog-pageid"), requests_session=session)
        result = run(client, request)
        print(json.dumps(result, ensure_ascii=True))
    except ImportError:
        print('{"type":"error","code":"runtime"}')
    except Exception:
        # Exceptions can include HTTP headers or request data: never serialize them.
        print('{"type":"error","code":"request"}')


if __name__ == "__main__":
    main()

"""Local HTTP server: JSON API + the static single-page UI. Standard library only."""

from __future__ import annotations

import json
import math
import mimetypes
import os
import threading
import traceback
from collections import OrderedDict
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

from .checkpoints import CheckpointCache, Run, discover
from .diff import spectrum, tensor_detail
from .jobs import PRIORITY_BACKGROUND, PRIORITY_INTERACTIVE, PairStore, pair_key

STATIC_DIR = os.path.join(os.path.dirname(__file__), "static")


class ApiError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(message)
        self.status = status


def _clean(obj):
    """JSON-safe copy: NaN/inf -> None (browsers reject bare NaN)."""
    if isinstance(obj, float):
        return obj if math.isfinite(obj) else None
    if isinstance(obj, dict):
        return {k: _clean(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_clean(v) for v in obj]
    return obj


class _LRU:
    def __init__(self, size: int):
        self.size, self._d, self._lock = size, OrderedDict(), threading.Lock()

    def get(self, key):
        with self._lock:
            if key in self._d:
                self._d.move_to_end(key)
                return self._d[key]
        return None

    def put(self, key, value):
        with self._lock:
            self._d[key] = value
            while len(self._d) > self.size:
                self._d.popitem(last=False)


class App:
    def __init__(self, root: str, cache_dir: str, workers: int | None = None):
        self.root = os.path.abspath(root)
        self.checkpoints = CheckpointCache()
        self.pairs = PairStore(cache_dir, self.checkpoints, workers)
        self.details = _LRU(64)
        self._runs: dict[str, Run] = {}
        self._runs_lock = threading.Lock()
        self.rescan()

    def rescan(self) -> list[Run]:
        runs = discover(self.root)
        with self._runs_lock:
            self._runs = {r.name: r for r in runs}
        return runs

    def run(self, name: str | None) -> Run:
        with self._runs_lock:
            run = self._runs.get(name or "")
        if run is None:
            self.rescan()
            with self._runs_lock:
                run = self._runs.get(name or "")
        if run is None:
            raise ApiError(404, f"unknown run {name!r}")
        return run

    def days(self, run: Run, a: str | None, b: str | None):
        for attempt in range(2):
            try:
                return run.day(a or ""), run.day(b or "")
            except KeyError as e:
                if attempt:
                    raise ApiError(404, str(e)) from None
                self.rescan()  # a new day may have landed since the last scan
                run = self.run(run.name)

    # -- endpoints ---------------------------------------------------------------

    def api_runs(self, q):
        runs = self.rescan()
        return {"root": self.root, "runs": [r.to_json() for r in runs]}

    def api_timeline(self, q):
        run = self.run(q.get("run"))
        days = run.ready_days
        mode = q.get("mode", "consecutive")
        if mode == "baseline":
            pairs = [(days[0], d) for d in days[1:]]
        else:
            pairs = list(zip(days, days[1:]))
        # Schedule newest first, so the most recent day's diff is ready soonest.
        statuses = [self.pairs.status(a, b, PRIORITY_BACKGROUND) for a, b in reversed(pairs)][::-1]
        out = [{"a": a.name, "b": b.name, **st} for (a, b), st in zip(pairs, statuses)]
        return {"run": run.name, "mode": mode, "pairs": out}

    def api_pair(self, q):
        run = self.run(q.get("run"))
        a, b = self.days(run, q.get("a"), q.get("b"))
        return self.pairs.request(a, b, PRIORITY_INTERACTIVE)

    def _detail_args(self, q):
        run = self.run(q.get("run"))
        a, b = self.days(run, q.get("a"), q.get("b"))
        name = q.get("name") or ""
        for day in (a, b):
            if day.error:
                raise ApiError(409, f"{day.name}: {day.error}")
        ck_a, ck_b = self.checkpoints.get(a), self.checkpoints.get(b)
        if name not in ck_a.tensors or name not in ck_b.tensors:
            raise ApiError(404, f"tensor {name!r} is not in both checkpoints")
        return pair_key(a, b), ck_a, ck_b, name

    def api_tensor(self, q):
        key, ck_a, ck_b, name = self._detail_args(q)
        grid = max(16, min(512, int(q.get("grid") or 256)))
        cache_key = ("tensor", key, name, grid)
        out = self.details.get(cache_key)
        if out is None:
            try:
                out = tensor_detail(ck_a, ck_b, name, grid=grid)
            except ValueError as e:
                raise ApiError(409, str(e)) from None
            self.details.put(cache_key, out)
        return out

    def api_spectrum(self, q):
        key, ck_a, ck_b, name = self._detail_args(q)
        k = max(1, min(128, int(q.get("k") or 32)))
        cache_key = ("spectrum", key, name, k)
        out = self.details.get(cache_key)
        if out is None:
            try:
                out = spectrum(ck_a, ck_b, name, k=k)
            except ValueError as e:
                raise ApiError(409, str(e)) from None
            self.details.put(cache_key, out)
        return out


def make_handler(app: App):
    routes = {
        "/api/runs": app.api_runs,
        "/api/timeline": app.api_timeline,
        "/api/pair": app.api_pair,
        "/api/tensor": app.api_tensor,
        "/api/spectrum": app.api_spectrum,
    }
    static_files = {f for f in os.listdir(STATIC_DIR) if not f.startswith(".")}

    class Handler(BaseHTTPRequestHandler):
        server_version = "weightviz"

        def log_request(self, code="-", size="-"):  # the UI polls; only log server errors
            if isinstance(code, int) and code >= 500:
                super().log_request(code, size)

        def _send(self, status: int, body: bytes, ctype: str, cache: bool = False):
            self.send_response(status)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "max-age=3600" if cache else "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.end_headers()
            self.wfile.write(body)

        def _json(self, status: int, obj):
            body = json.dumps(_clean(obj), separators=(",", ":"), allow_nan=False).encode()
            self._send(status, body, "application/json")

        def do_GET(self):  # noqa: N802
            url = urlparse(self.path)
            if url.path in routes:
                q = {k: v[-1] for k, v in parse_qs(url.query).items()}
                try:
                    self._json(200, routes[url.path](q))
                except ApiError as e:
                    self._json(e.status, {"error": str(e)})
                except Exception as e:
                    traceback.print_exc()
                    self._json(500, {"error": f"{type(e).__name__}: {e}"})
                return
            name = "index.html" if url.path in ("/", "/index.html") else url.path.removeprefix("/static/")
            if name not in static_files:  # whitelist: no path traversal possible
                self._send(404, b"not found", "text/plain")
                return
            with open(os.path.join(STATIC_DIR, name), "rb") as f:
                body = f.read()
            ctype = mimetypes.guess_type(name)[0] or "application/octet-stream"
            if ctype.startswith("text/") or ctype.endswith("javascript"):
                ctype += "; charset=utf-8"
            self._send(200, body, ctype)

    return Handler


def serve(root: str, host: str, port: int, cache_dir: str, workers: int | None = None) -> None:
    app = App(root, cache_dir, workers)
    httpd = ThreadingHTTPServer((host, port), make_handler(app))
    httpd.daemon_threads = True
    runs = app.rescan()
    print(f"weightviz: watching {app.root}")
    for r in runs:
        ready = len(r.ready_days)
        print(f"  run {r.name!r}: {len(r.days)} day(s), {ready} readable")
    if not runs:
        print("  (no runs found yet — see README 'Checkpoint convention')")
    print(f"  cache: {app.pairs.cache_dir}")
    print(f"open http://{host}:{httpd.server_address[1]}/")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()

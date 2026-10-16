"""Background computation and caching of day-pair diffs.

A full pair diff reads both checkpoints end to end (two 15 GB reads for a 7B
bf16 model), so results are computed on a worker thread and cached on disk,
keyed by the checkpoint files' names, sizes and mtimes. Re-writing a day's
checkpoint invalidates its cached diffs automatically.
"""

from __future__ import annotations

import hashlib
import itertools
import json
import os
import queue
import threading
import traceback

from .checkpoints import CheckpointCache, Day
from .diff import compute_pair

CACHE_VERSION = 1

PRIORITY_INTERACTIVE = 0  # a pair the user is looking at right now
PRIORITY_BACKGROUND = 1  # timeline fill-in


def pair_key(a: Day, b: Day) -> str:
    raw = f"v{CACHE_VERSION}|{a.path}|{a.fingerprint}|{b.path}|{b.fingerprint}"
    return hashlib.sha1(raw.encode()).hexdigest()[:20]


class PairStore:
    def __init__(self, cache_dir: str, checkpoints: CheckpointCache, workers: int | None = None):
        self.cache_dir = os.path.join(cache_dir, "pairs")
        os.makedirs(self.cache_dir, exist_ok=True)
        self.checkpoints = checkpoints
        self.workers = workers
        self._lock = threading.Lock()
        self._results: dict[str, dict] = {}
        self._state: dict[str, dict] = {}  # key -> {"state", "progress", "error"}
        self._days: dict[str, tuple[Day, Day]] = {}
        self._queue: queue.PriorityQueue = queue.PriorityQueue()
        self._seq = itertools.count()
        threading.Thread(target=self._worker, name="pair-worker", daemon=True).start()

    # -- public ------------------------------------------------------------------

    def request(self, a: Day, b: Day, priority: int = PRIORITY_BACKGROUND) -> dict:
        """Status of the A->B diff, scheduling it if needed. Includes 'result' when done."""
        key = pair_key(a, b)
        for day in (a, b):
            if day.error:
                return {"key": key, "state": "error", "error": f"{day.name}: {day.error}"}
        with self._lock:
            if key in self._results:
                return {"key": key, "state": "done", "result": self._results[key]}
            state = self._state.get(key)
        if state is None or state["state"] == "queued":
            cached = self._load(key)
            if cached is not None:
                with self._lock:
                    self._results[key] = cached
                    self._state.pop(key, None)
                return {"key": key, "state": "done", "result": cached}
        with self._lock:
            state = self._state.get(key)
            if state is None or (state["state"] == "queued" and priority < state["priority"]):
                self._state[key] = {"state": "queued", "progress": 0.0, "priority": priority}
                self._days[key] = (a, b)
                self._queue.put((priority, next(self._seq), key))
                state = self._state[key]
            return {"key": key, **{k: v for k, v in state.items() if k != "priority"}}

    def status(self, a: Day, b: Day, priority: int = PRIORITY_BACKGROUND) -> dict:
        """Like request() but without the (potentially large) result payload."""
        out = self.request(a, b, priority)
        out.pop("result", None)
        return out

    # -- internals ---------------------------------------------------------------

    def _path(self, key: str) -> str:
        return os.path.join(self.cache_dir, f"{key}.json")

    def _load(self, key: str) -> dict | None:
        try:
            with open(self._path(key)) as f:
                return json.load(f)
        except (OSError, json.JSONDecodeError):
            return None

    def _save(self, key: str, result: dict) -> None:
        tmp = self._path(key) + f".tmp{os.getpid()}"
        try:
            with open(tmp, "w") as f:
                json.dump(result, f, separators=(",", ":"))
            os.replace(tmp, self._path(key))
        except OSError:
            pass  # cache is best-effort (e.g. read-only home)

    def _set(self, key: str, **fields) -> None:
        with self._lock:
            self._state.setdefault(key, {}).update(fields)

    def _worker(self) -> None:
        while True:
            _, _, key = self._queue.get()
            with self._lock:
                state = self._state.get(key)
                if key in self._results or state is None or state["state"] != "queued":
                    continue  # duplicate queue entry (re-prioritised) or already finished
                a, b = self._days[key]
                state["state"] = "running"
            try:
                result = compute_pair(
                    self.checkpoints.get(a),
                    self.checkpoints.get(b),
                    progress=lambda p, key=key: self._set(key, progress=round(p, 4)),
                    workers=self.workers,
                )
                result["key"] = key
                self._save(key, result)
                with self._lock:
                    self._results[key] = result
                    self._state.pop(key, None)
            except Exception as e:  # surface to the UI instead of killing the worker
                traceback.print_exc()
                self._set(key, state="error", error=f"{type(e).__name__}: {e}")

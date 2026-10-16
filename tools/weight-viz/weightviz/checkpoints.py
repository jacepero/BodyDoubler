"""Discover Greenhouse runs and daily checkpoints on disk.

Expected layout (see README "Checkpoint convention"):

    <root>/
      <run>/
        2026-10-14/
          model.safetensors                  # or sharded: model-00001-of-00004.safetensors ...
          model.safetensors.index.json       # (sharded only) HF-style weight_map
          meta.json                          # optional: {"step": ..., "loss": ..., "notes": ...}
        2026-10-15/
          ...

`<root>` may also *be* a single run (its children are the day folders). Day
folders sort naturally by name, so ISO dates (or step-000123) order correctly.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import threading
from collections import OrderedDict
from dataclasses import dataclass, field

from .safetensors_io import SafetensorsError, SafetensorsFile, TensorInfo, read_header

INDEX_NAME = "model.safetensors.index.json"
META_NAME = "meta.json"


def natural_key(s: str):
    return [int(p) if p.isdigit() else p.lower() for p in re.split(r"(\d+)", s)]


def _visible_dirs(path: str) -> list[str]:
    try:
        names = os.listdir(path)
    except OSError:
        return []
    return sorted(
        (n for n in names if not n.startswith(".") and os.path.isdir(os.path.join(path, n))),
        key=natural_key,
    )


def checkpoint_files(day_dir: str) -> list[str]:
    """The safetensors files that make up the model weights in a day folder."""
    index_path = os.path.join(day_dir, INDEX_NAME)
    if os.path.isfile(index_path):
        with open(index_path) as f:
            weight_map = json.load(f).get("weight_map", {})
        names = sorted(set(weight_map.values()), key=natural_key)
        return [os.path.join(day_dir, n) for n in names if os.path.basename(n) == n]
    try:
        files = sorted((n for n in os.listdir(day_dir) if n.endswith(".safetensors")), key=natural_key)
    except OSError:
        return []
    # Ignore optimizer/scheduler state if it sits next to the weights.
    model_files = [n for n in files if n.startswith("model")]
    return [os.path.join(day_dir, n) for n in (model_files or files)]


@dataclass
class Day:
    name: str
    path: str
    files: list[str]
    meta: dict = field(default_factory=dict)
    error: str | None = None
    n_tensors: int = 0
    n_params: int = 0
    nbytes: int = 0
    fingerprint: str = ""

    def to_json(self) -> dict:
        return {
            "name": self.name,
            "meta": self.meta,
            "error": self.error,
            "n_tensors": self.n_tensors,
            "n_params": self.n_params,
            "nbytes": self.nbytes,
            "fingerprint": self.fingerprint,
        }


@dataclass
class Run:
    name: str
    path: str
    days: list[Day]

    def day(self, name: str) -> Day:
        for d in self.days:
            if d.name == name:
                return d
        raise KeyError(f"run {self.name!r} has no day {name!r}")

    @property
    def ready_days(self) -> list[Day]:
        return [d for d in self.days if d.error is None]

    def to_json(self) -> dict:
        return {"name": self.name, "days": [d.to_json() for d in self.days]}


def _load_day(name: str, path: str) -> Day:
    day = Day(name=name, path=path, files=checkpoint_files(path))
    meta_path = os.path.join(path, META_NAME)
    if os.path.isfile(meta_path):
        try:
            with open(meta_path) as f:
                meta = json.load(f)
            day.meta = meta if isinstance(meta, dict) else {}
        except (OSError, json.JSONDecodeError):
            day.meta = {}
    if not day.files:
        day.error = "no .safetensors files"
        return day
    h = hashlib.sha1()
    try:
        seen: set[str] = set()
        for fp in day.files:
            st = os.stat(fp)
            h.update(f"{os.path.basename(fp)}:{st.st_size}:{st.st_mtime_ns};".encode())
            tensors, _ = read_header(fp)
            for t in tensors.values():
                if t.name in seen:
                    raise SafetensorsError(f"tensor {t.name} appears in more than one file")
                seen.add(t.name)
                day.n_params += t.numel
                day.nbytes += t.nbytes
            day.n_tensors += len(tensors)
    except (OSError, SafetensorsError) as e:
        day.error = str(e)
    day.fingerprint = h.hexdigest()
    return day


def _is_day_dir(path: str) -> bool:
    return bool(checkpoint_files(path))


def discover(root: str) -> list[Run]:
    """Find runs under root. Cheap: reads only file headers."""
    root = os.path.abspath(root)
    children = _visible_dirs(root)
    if any(_is_day_dir(os.path.join(root, c)) for c in children):
        candidates = [(os.path.basename(root) or root, root)]
    else:
        candidates = [(c, os.path.join(root, c)) for c in children]
    runs = []
    for run_name, run_path in candidates:
        day_names = [d for d in _visible_dirs(run_path) if _is_day_dir(os.path.join(run_path, d))]
        if day_names:
            runs.append(Run(run_name, run_path, [_load_day(d, os.path.join(run_path, d)) for d in day_names]))
    return runs


class Checkpoint:
    """All tensors of one day, across shards, memory-mapped."""

    def __init__(self, day: Day):
        self.day = day
        self.files = [SafetensorsFile(p) for p in day.files]
        self.tensors: dict[str, tuple[SafetensorsFile, TensorInfo]] = {}
        for sf in self.files:
            for name, info in sf.tensors.items():
                self.tensors[name] = (sf, info)

    def info(self, name: str) -> TensorInfo:
        return self.tensors[name][1]

    def raw(self, name: str):
        sf, _ = self.tensors[name]
        return sf.raw(name)


class CheckpointCache:
    """Keeps a few recently used Checkpoints open (memmaps are cheap but not free)."""

    def __init__(self, size: int = 8):
        self.size = size
        self._lock = threading.Lock()
        self._items: OrderedDict[str, Checkpoint] = OrderedDict()

    def get(self, day: Day) -> Checkpoint:
        key = f"{day.path}|{day.fingerprint}"
        with self._lock:
            ck = self._items.get(key)
            if ck is not None:
                self._items.move_to_end(key)
                return ck
        ck = Checkpoint(day)
        with self._lock:
            self._items[key] = ck
            while len(self._items) > self.size:
                self._items.popitem(last=False)
        return ck

"""Training-side helper: write a daily checkpoint in the layout weightviz reads.

    from weightviz.greenhouse import save_daily_checkpoint

    # once a day (or at any cadence you like), from rank 0 after gathering a full state dict:
    save_daily_checkpoint(
        model.state_dict(),
        root="/data/greenhouse/checkpoints",
        run="qwen3-1.7b-sft",
        meta={"step": step, "tokens": tokens_seen, "loss": eval_loss, "lr": lr},
    )

Writes <root>/<run>/<YYYY-MM-DD>/model.safetensors (sharded with an HF-style
index above `max_shard_bytes`) plus meta.json. The folder is built under a
hidden temporary name and renamed into place, so the visualizer never sees a
half-written day. Tensors may be torch tensors (any device; bf16/fp8 kept
bit-exact), numpy arrays, or (raw_bits, "BF16") tuples. Needs only numpy (+ torch if you pass torch tensors).
"""

from __future__ import annotations

import datetime as _dt
import json
import os
import shutil

import numpy as np

from .safetensors_io import save_file

INDEX_NAME = "model.safetensors.index.json"


def _nbytes(t) -> int:
    if isinstance(t, tuple):  # (raw_bits, dtype_tag), already encoded
        return t[0].nbytes
    if isinstance(t, np.ndarray):
        return t.nbytes
    return t.numel() * t.element_size()  # torch.Tensor


def _to_numpy(t):
    """numpy array, or (raw_bits, dtype_tag) for dtypes numpy lacks."""
    if isinstance(t, (np.ndarray, tuple)):
        return t
    import torch  # only reached when torch tensors are passed

    t = t.detach().to("cpu").contiguous()
    if t.dtype == torch.bfloat16:
        return t.view(torch.int16).numpy().view(np.uint16), "BF16"
    for name, tag in (("float8_e4m3fn", "F8_E4M3"), ("float8_e5m2", "F8_E5M2")):
        if t.dtype == getattr(torch, name, None):
            return t.view(torch.uint8).numpy(), tag
    return t.numpy()


def _jsonable(x):
    if hasattr(x, "item"):
        return x.item()
    if isinstance(x, (_dt.date, _dt.datetime)):
        return x.isoformat()
    return str(x)


def plan_shards(sizes: dict[str, int], max_shard_bytes: int) -> list[list[str]]:
    shards: list[list[str]] = [[]]
    used = 0
    for name, size in sizes.items():
        if shards[-1] and used + size > max_shard_bytes:
            shards.append([])
            used = 0
        shards[-1].append(name)
        used += size
    return shards


def save_daily_checkpoint(
    tensors,
    root: str,
    run: str,
    day: str | None = None,
    meta: dict | None = None,
    max_shard_bytes: int = 4 << 30,
    overwrite: bool = True,
) -> str:
    """Save `tensors` (a state dict) as <root>/<run>/<day>/. Returns the folder path."""
    day = day or _dt.date.today().isoformat()
    for part in (run, day):
        if not part or part.startswith(".") or os.sep in part or (os.altsep and os.altsep in part):
            raise ValueError(f"invalid run/day name {part!r}")
    run_dir = os.path.join(root, run)
    final = os.path.join(run_dir, day)
    if os.path.exists(final) and not overwrite:
        raise FileExistsError(final)
    tmp = os.path.join(run_dir, f".{day}.tmp-{os.getpid()}")  # hidden: ignored by discovery
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    try:
        tensors = dict(tensors)
        sizes = {name: _nbytes(t) for name, t in tensors.items()}
        shards = plan_shards(sizes, max_shard_bytes)
        weight_map = {}
        for i, names in enumerate(shards):
            fname = "model.safetensors" if len(shards) == 1 else f"model-{i + 1:05d}-of-{len(shards):05d}.safetensors"
            # Convert one shard at a time to bound host memory.
            save_file({n: _to_numpy(tensors[n]) for n in names}, os.path.join(tmp, fname), metadata={"format": "pt"})
            weight_map.update(dict.fromkeys(names, fname))
        if len(shards) > 1:
            with open(os.path.join(tmp, INDEX_NAME), "w") as f:
                json.dump({"metadata": {"total_size": sum(sizes.values())}, "weight_map": weight_map}, f, indent=1)
        info = {"day": day, "saved_at": _dt.datetime.now().astimezone().isoformat(timespec="seconds"), **(meta or {})}
        with open(os.path.join(tmp, "meta.json"), "w") as f:
            json.dump(info, f, indent=2, default=_jsonable)

        old = None
        if os.path.exists(final):
            old = os.path.join(run_dir, f".{day}.old-{os.getpid()}")
            os.replace(final, old)
        os.replace(tmp, final)
        if old:
            shutil.rmtree(old, ignore_errors=True)
    except BaseException:
        shutil.rmtree(tmp, ignore_errors=True)
        raise
    return final

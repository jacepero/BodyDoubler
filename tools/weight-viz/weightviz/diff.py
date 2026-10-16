"""Weight-diff math: per-tensor statistics, downsampled delta maps, histograms, spectra.

Everything streams over memory-mapped tensors in chunks of CHUNK elements, so
peak memory stays around a few hundred MB regardless of model size.

Per-tensor statistics are stored as additive sums (||A||^2, ||B||^2, ||D||^2,
...) so that any grouping (a layer, a module type, the whole model) can be
aggregated exactly by summing, and ratios such as relative change
||D|| / ||A|| are derived afterwards. Cosine distance is derived as
(||D||^2 - (||B|| - ||A||)^2) / (2 ||A|| ||B||), which stays accurate for the
tiny day-to-day updates where 1 - A.B/(||A|| ||B||) would cancel catastrophically.
Sums use numpy's pairwise summation (np.sum) rather than BLAS dot, for accuracy.
"""

from __future__ import annotations

import base64
import math
import os
import threading
import time
from concurrent.futures import ThreadPoolExecutor

import numpy as np

from .checkpoints import Checkpoint
from .layout import describe
from .safetensors_io import TensorInfo, decode

CHUNK = 1 << 22  # elements per streamed chunk (16 MB as float32)
STAT_FIELDS = ("numel", "sq_a", "sq_b", "sq_d", "sum_d", "maxabs", "changed", "nonfinite")


def _sumsq(x: np.ndarray) -> float:
    return float(np.square(x).sum())


def cosine(sq_a: float, sq_b: float, sq_d: float) -> float | None:
    if sq_a <= 0 or sq_b <= 0:
        return None
    na, nb = math.sqrt(sq_a), math.sqrt(sq_b)
    return 1.0 - max(0.0, sq_d - (nb - na) ** 2) / (2 * na * nb)


def _matrix_shape(shape: tuple[int, ...]) -> tuple[int, int]:
    """View any tensor as a 2-D matrix: (prod(leading dims), last dim)."""
    if len(shape) == 0:
        return 1, 1
    if len(shape) == 1:
        return 1, shape[0]
    return math.prod(shape[:-1]), shape[-1]


def _flat_chunks(ck_a: Checkpoint, ck_b: Checkpoint, name: str, chunk: int = CHUNK):
    ia, ib = ck_a.info(name), ck_b.info(name)
    ra, rb = ck_a.raw(name), ck_b.raw(name)
    for s in range(0, ra.size, chunk):
        yield decode(ra[s : s + chunk], ia.dtype), decode(rb[s : s + chunk], ib.dtype)


def _row_chunks(ck_a: Checkpoint, ck_b: Checkpoint, name: str, rows_per_chunk: int):
    """Yield (first_row, A_rows, B_rows) as float32 (rows, C) blocks."""
    ia, ib = ck_a.info(name), ck_b.info(name)
    R, C = _matrix_shape(ia.shape)
    ra, rb = ck_a.raw(name), ck_b.raw(name)
    for r0 in range(0, R, rows_per_chunk):
        r1 = min(R, r0 + rows_per_chunk)
        a = decode(ra[r0 * C : r1 * C], ia.dtype).reshape(r1 - r0, C)
        b = decode(rb[r0 * C : r1 * C], ib.dtype).reshape(r1 - r0, C)
        yield r0, a, b


# --------------------------------------------------------------------------- pair stats


def tensor_stats(ck_a: Checkpoint, ck_b: Checkpoint, name: str, on_bytes=None) -> dict:
    """Additive diff statistics for one tensor present (with equal shape) in both."""
    ia = ck_a.info(name)
    acc = dict.fromkeys(STAT_FIELDS, 0.0)
    acc["numel"] = ia.numel
    bytes_per_el = (ia.nbytes + ck_b.info(name).nbytes) / max(ia.numel, 1)
    for a, b in _flat_chunks(ck_a, ck_b, name):
        d = b - a
        sq_b = _sumsq(b)
        acc["sq_a"] += _sumsq(a)
        acc["sq_b"] += sq_b
        acc["sq_d"] += _sumsq(d)
        acc["sum_d"] += float(d.sum(dtype=np.float64))
        if d.size:
            acc["maxabs"] = max(acc["maxabs"], float(np.abs(d).max()))  # NaN is ignored by max()
        acc["changed"] += int(np.count_nonzero(d))
        if not math.isfinite(sq_b):
            acc["nonfinite"] += int(np.count_nonzero(~np.isfinite(b)))
        if on_bytes:
            on_bytes(a.size * bytes_per_el)
    return acc


def _ordered_names(names, layouts) -> list[str]:
    return sorted(names, key=lambda n: (layouts[n]["row_order"], layouts[n]["col_order"], n))


def compute_pair(ck_a: Checkpoint, ck_b: Checkpoint, progress=None, workers: int | None = None) -> dict:
    """Diff every tensor of day A against day B. Returns a columnar result."""
    t0 = time.time()
    names_a, names_b = set(ck_a.tensors), set(ck_b.tensors)
    all_names = names_a | names_b
    layouts = {n: describe(n) for n in all_names}
    names = _ordered_names(all_names, layouts)

    status: dict[str, str] = {}
    for n in names:
        if n not in names_a:
            status[n] = "added"
        elif n not in names_b:
            status[n] = "removed"
        elif ck_a.info(n).shape != ck_b.info(n).shape:
            status[n] = "reshaped"
        else:
            status[n] = "ok"

    ok = [n for n in names if status[n] == "ok"]
    total_bytes = sum(ck_a.info(n).nbytes + ck_b.info(n).nbytes for n in ok) or 1
    done = [0.0]
    lock = threading.Lock()

    def on_bytes(nb):
        with lock:
            done[0] += nb
            frac = done[0] / total_bytes
        if progress:
            progress(min(frac, 0.999))

    # Biggest tensors first so the pool doesn't end on one long straggler.
    ok_by_size = sorted(ok, key=lambda n: -ck_a.info(n).numel)
    workers = workers or min(8, os.cpu_count() or 2)
    with ThreadPoolExecutor(max_workers=workers) as pool:
        stats = dict(zip(ok_by_size, pool.map(lambda n: tensor_stats(ck_a, ck_b, n, on_bytes), ok_by_size)))

    def info(n) -> TensorInfo:
        return ck_b.info(n) if n in names_b else ck_a.info(n)

    cols: dict[str, list] = {
        "name": names,
        "shape": [list(info(n).shape) for n in names],
        "dtype": [info(n).dtype for n in names],
        "status": [status[n] for n in names],
    }
    for key in ("row", "row_label", "row_order", "module", "module_label", "group", "col_order"):
        cols[key] = [layouts[n][key] for n in names]
    for f in STAT_FIELDS:
        cols[f] = [stats[n][f] if n in stats else None for n in names]
    if progress:
        progress(1.0)
    return {"a": ck_a.day.name, "b": ck_b.day.name, "tensors": cols, "elapsed": round(time.time() - t0, 3)}


# --------------------------------------------------------------------------- tensor detail


def _b64(arr: np.ndarray) -> str:
    return base64.b64encode(np.ascontiguousarray(arr, dtype="<f4").tobytes()).decode()


def _sample_delta(ck_a: Checkpoint, ck_b: Checkpoint, name: str, target: int = 1_000_000) -> np.ndarray:
    ia, ib = ck_a.info(name), ck_b.info(name)
    ra, rb = ck_a.raw(name), ck_b.raw(name)
    stride = max(1, ra.size // target)
    return decode(rb[::stride], ib.dtype) - decode(ra[::stride], ia.dtype)


def _block_sum(x: np.ndarray, br: int, col_starts: np.ndarray) -> np.ndarray:
    """Sum a (rows, C) chunk over blocks of `br` rows and column blocks starting at col_starts."""
    n = x.shape[0]
    full = n // br * br
    parts = []
    if full:
        parts.append(x[:full].reshape(-1, br, x.shape[1]).sum(axis=1))
    if full < n:
        parts.append(x[full:].sum(axis=0, keepdims=True))
    rows = parts[0] if len(parts) == 1 else np.concatenate(parts)
    return np.add.reduceat(rows.astype(np.float64), col_starts, axis=1)


def _hist_add(counts: np.ndarray, v: np.ndarray, limit: float) -> tuple[int, int]:
    """Add v to a uniform histogram over [-limit, limit]; returns (n below, n above). NaN is skipped."""
    bins = counts.size
    inside = v[np.abs(v) <= limit]
    idx = ((inside + limit) * (bins / (2 * limit))).astype(np.intp)
    np.minimum(idx, bins - 1, out=idx)
    counts += np.bincount(idx, minlength=bins)
    return int(np.count_nonzero(v < -limit)), int(np.count_nonzero(v > limit))


def tensor_detail(ck_a: Checkpoint, ck_b: Checkpoint, name: str, grid: int = 256, bins: int = 81) -> dict:
    """Downsampled delta map + histogram + summary stats for one tensor. `bins` must be odd."""
    ia, ib = ck_a.info(name), ck_b.info(name)
    if ia.shape != ib.shape:
        raise ValueError(f"{name}: shape changed {ia.shape} -> {ib.shape}")
    R, C = _matrix_shape(ia.shape)
    one_d = R == 1
    gr_target = 1 if one_d else min(R, grid)
    gc_target = min(C, grid * 2 if one_d else grid)
    br, bc = math.ceil(R / gr_target), math.ceil(C / gc_target)
    gr, gc = math.ceil(R / br), math.ceil(C / bc)
    col_starts = np.arange(0, C, bc)
    col_sizes = np.diff(np.append(col_starts, C)).astype(np.float64)

    sum_d = np.zeros((gr, gc))
    sum_d2 = np.zeros((gr, gc))
    sum_b2 = np.zeros((gr, gc))
    min_d = np.full((gr, gc), np.inf) if one_d else None
    max_d = np.full((gr, gc), -np.inf) if one_d else None
    row_counts = np.zeros(gr)

    # Histogram range from a strided sample: bulk of non-zero |delta|, outliers counted separately.
    sample = _sample_delta(ck_a, ck_b, name)
    nz = np.abs(sample[(sample != 0) & np.isfinite(sample)])
    limit = float(np.quantile(nz, 0.999) * 1.05) if nz.size else 0.0
    counts = np.zeros(bins, dtype=np.int64)
    below = above = zeros = 0
    tot = dict(sq_a=0.0, sq_b=0.0, sq_d=0.0, maxabs=0.0)

    rows_per_chunk = max(br, (CHUNK // max(C, 1)) // br * br)
    for r0, a, b in _row_chunks(ck_a, ck_b, name, rows_per_chunk):
        d = b - a
        d2 = d * d
        b2 = b * b
        g0 = r0 // br
        g1 = g0 + math.ceil(d.shape[0] / br)
        sum_d[g0:g1] += _block_sum(d, br, col_starts)
        sum_d2[g0:g1] += _block_sum(d2, br, col_starts)
        sum_b2[g0:g1] += _block_sum(b2, br, col_starts)
        row_counts[g0:g1] += np.diff(np.append(np.arange(0, d.shape[0], br), d.shape[0]))
        if one_d:
            min_d[g0:g1] = np.minimum(min_d[g0:g1], np.minimum.reduceat(d, col_starts, axis=1))
            max_d[g0:g1] = np.maximum(max_d[g0:g1], np.maximum.reduceat(d, col_starts, axis=1))
        tot["sq_a"] += _sumsq(a)
        tot["sq_b"] += float(b2.sum())
        tot["sq_d"] += float(d2.sum())
        flat = d.ravel()
        m = float(np.abs(flat).max()) if flat.size else 0.0
        if not math.isfinite(m):
            fin = flat[np.isfinite(flat)]
            m = float(np.abs(fin).max()) if fin.size else 0.0
        tot["maxabs"] = max(tot["maxabs"], m)
        zeros += int(flat.size - np.count_nonzero(flat))
        if limit > 0:
            lo, hi = _hist_add(counts, flat, limit)
            below += lo
            above += hi

    if limit == 0 and tot["maxabs"] > 0:
        # Sample saw no change but the full pass did (sparse update): second pass for the histogram.
        limit = tot["maxabs"]
        for a, b in _flat_chunks(ck_a, ck_b, name):
            _hist_add(counts, b - a, limit)
    if limit > 0:
        counts[bins // 2] -= zeros  # exact zeros land in the centre bin; report them separately instead
    edges = np.linspace(-limit, limit, bins + 1) if limit > 0 else None

    cell_counts = np.outer(row_counts, col_sizes)
    numel = ia.numel
    out = {
        "name": name,
        "shape": list(ia.shape),
        "dtype_a": ia.dtype,
        "dtype_b": ib.dtype,
        "matrix": [R, C],
        "grid": {
            "rows": gr,
            "cols": gc,
            "block": [br, bc],
            "delta_mean": _b64(sum_d / cell_counts),
            "delta_rms": _b64(np.sqrt(sum_d2 / cell_counts)),
            "weight_rms": _b64(np.sqrt(sum_b2 / cell_counts)),
        },
        "hist": {
            "edges": edges.tolist() if edges is not None else [],
            "counts": counts.tolist(),
            "below": below,
            "above": above,
            "zeros": zeros,
        },
        "stats": {
            "numel": numel,
            "norm_a": math.sqrt(tot["sq_a"]),
            "norm_b": math.sqrt(tot["sq_b"]),
            "norm_d": math.sqrt(tot["sq_d"]),
            "rel": math.sqrt(tot["sq_d"] / tot["sq_a"]) if tot["sq_a"] > 0 else None,
            "cos": cosine(tot["sq_a"], tot["sq_b"], tot["sq_d"]),
            "rms_d": math.sqrt(tot["sq_d"] / numel) if numel else 0.0,
            "maxabs": tot["maxabs"],
            "changed_frac": (numel - zeros) / numel if numel else 0.0,
        },
    }
    if one_d:
        out["grid"]["delta_min"] = _b64(min_d)
        out["grid"]["delta_max"] = _b64(max_d)
    return out


# --------------------------------------------------------------------------- spectrum


def spectrum(ck_a: Checkpoint, ck_b: Checkpoint, name: str, k: int = 32, power_iters: int | None = None, seed: int = 0) -> dict:
    """Top-k singular values of the update D = B - A (randomized SVD, streamed).

    A low-rank update (a few directions carry most of the energy) looks very
    different from diffuse full-rank drift; share[i] = s_i^2 / ||D||_F^2.
    """
    ia, ib = ck_a.info(name), ck_b.info(name)
    if ia.shape != ib.shape:
        raise ValueError(f"{name}: shape changed")
    R, C = _matrix_shape(ia.shape)
    if len(ia.shape) < 2 or min(R, C) < 2:
        return {"name": name, "values": [], "share": [], "total": 0.0, "method": "n/a"}
    if power_iters is None:  # each iteration is two more passes over the tensor
        power_iters = 1 if R * C > 100_000_000 else 2
    rows_per_chunk = max(1, CHUNK // C)
    chunks = lambda: _row_chunks(ck_a, ck_b, name, rows_per_chunk)  # noqa: E731

    if R * C <= 1 << 20:
        d = np.concatenate([b - a for _, a, b in chunks()], axis=0).astype(np.float64)
        total = float(np.sum(d * d))
        s = np.linalg.svd(d, compute_uv=False)[:k]
        method = "exact"
    else:
        l = min(k + 8, R, C)
        omega = np.random.default_rng(seed).standard_normal((C, l)).astype(np.float32)
        total = 0.0

        def times(x):  # D @ x
            nonlocal total
            y = np.empty((R, x.shape[1]), np.float32)
            tsum = 0.0
            for r0, a, b in chunks():
                d = b - a
                y[r0 : r0 + d.shape[0]] = d @ x
                tsum += float(np.einsum("ij,ij->", d, d, dtype=np.float64))
            total = tsum
            return y

        def t_times(y):  # D.T @ y
            z = np.zeros((C, y.shape[1]), np.float32)
            for r0, a, b in chunks():
                d = b - a
                z += d.T @ y[r0 : r0 + d.shape[0]]
            return z

        y = times(omega)
        for _ in range(power_iters):
            q, _ = np.linalg.qr(y)
            z, _ = np.linalg.qr(t_times(q))
            y = times(z)
        q, _ = np.linalg.qr(y)
        s = np.linalg.svd(t_times(q).astype(np.float64), compute_uv=False)[:k]
        method = "randomized"

    s = np.asarray(s, dtype=np.float64)
    share = (s * s / total).tolist() if total > 0 else [0.0] * len(s)
    return {"name": name, "values": s.tolist(), "share": share, "total": total, "method": method}

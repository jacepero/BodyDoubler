"""Minimal, dependency-free safetensors reader/writer built on numpy.

Why not the `safetensors` package? Its numpy backend can't represent bfloat16
(numpy has no bf16 dtype), and nearly every LLM checkpoint is bf16. The format
is simple enough to read directly:

    [u64 little-endian header length N][N bytes of JSON header][raw tensor bytes]

The header maps tensor name -> {"dtype", "shape", "data_offsets": [begin, end]},
with offsets relative to the first byte after the header. Files are
memory-mapped, so opening a 15 GB checkpoint costs nothing until tensors are
actually read, and reads go through the OS page cache.
"""

from __future__ import annotations

import json
import os
import struct
from dataclasses import dataclass

import numpy as np

# safetensors dtype tag -> numpy dtype used for the *raw* (undecoded) view.
_RAW_DTYPES = {
    "F64": np.dtype("<f8"),
    "F32": np.dtype("<f4"),
    "F16": np.dtype("<f2"),
    "BF16": np.dtype("<u2"),  # decoded to float32 by bit-shifting
    "F8_E4M3": np.dtype("u1"),  # decoded via lookup table
    "F8_E5M2": np.dtype("u1"),  # decoded via float16 reinterpretation
    "I64": np.dtype("<i8"),
    "I32": np.dtype("<i4"),
    "I16": np.dtype("<i2"),
    "I8": np.dtype("i1"),
    "U64": np.dtype("<u8"),
    "U32": np.dtype("<u4"),
    "U16": np.dtype("<u2"),
    "U8": np.dtype("u1"),
    "BOOL": np.dtype("?"),
}

FLOAT_DTYPES = {"F64", "F32", "F16", "BF16", "F8_E4M3", "F8_E5M2"}


class SafetensorsError(ValueError):
    pass


def _e4m3fn_table() -> np.ndarray:
    """float32 value of every float8 e4m3fn bit pattern (no infinities)."""
    bits = np.arange(256, dtype=np.uint32)
    sign = np.where(bits & 0x80, -1.0, 1.0)
    exp = ((bits >> 3) & 0xF).astype(np.int32)
    man = (bits & 0x7).astype(np.float64)
    normal = sign * np.ldexp(1.0 + man / 8.0, exp - 7)
    subnormal = sign * np.ldexp(man / 8.0, -6)
    out = np.where(exp == 0, subnormal, normal)
    out[(exp == 15) & (man == 7)] = np.nan
    return out.astype(np.float32)


_E4M3_TABLE = _e4m3fn_table()


def decode(raw: np.ndarray, dtype: str) -> np.ndarray:
    """Convert a raw view (see _RAW_DTYPES) to float32. Copies."""
    if dtype == "BF16":
        return (raw.astype(np.uint32) << 16).view(np.float32)
    if dtype == "F8_E4M3":
        return _E4M3_TABLE[raw]
    if dtype == "F8_E5M2":
        return (raw.astype(np.uint16) << 8).view(np.float16).astype(np.float32)
    return raw.astype(np.float32)


@dataclass(frozen=True)
class TensorInfo:
    name: str
    dtype: str
    shape: tuple[int, ...]
    begin: int  # absolute byte offset in file
    end: int

    @property
    def numel(self) -> int:
        n = 1
        for s in self.shape:
            n *= s
        return n

    @property
    def nbytes(self) -> int:
        return self.end - self.begin


def read_header(path: str) -> tuple[dict[str, TensorInfo], dict[str, str]]:
    """Parse a safetensors header without touching tensor data."""
    size = os.path.getsize(path)
    with open(path, "rb") as f:
        prefix = f.read(8)
        if len(prefix) < 8:
            raise SafetensorsError(f"{path}: file too small")
        (n,) = struct.unpack("<Q", prefix)
        if n > 100_000_000 or 8 + n > size:
            raise SafetensorsError(f"{path}: bad header length {n}")
        try:
            header = json.loads(f.read(n))
        except json.JSONDecodeError as e:
            raise SafetensorsError(f"{path}: header is not valid JSON ({e})") from e
    base = 8 + n
    metadata = header.pop("__metadata__", None) or {}
    tensors: dict[str, TensorInfo] = {}
    for name, entry in header.items():
        dtype = entry["dtype"]
        if dtype not in _RAW_DTYPES:
            raise SafetensorsError(f"{path}: {name} has unsupported dtype {dtype}")
        shape = tuple(int(s) for s in entry["shape"])
        begin, end = (int(x) for x in entry["data_offsets"])
        info = TensorInfo(name, dtype, shape, base + begin, base + end)
        if info.nbytes != info.numel * _RAW_DTYPES[dtype].itemsize:
            raise SafetensorsError(f"{path}: {name} byte size does not match its shape")
        if info.end > size:
            # Typical when a checkpoint is still being written.
            raise SafetensorsError(f"{path}: truncated ({name} extends past end of file)")
        tensors[name] = info
    return tensors, metadata


class SafetensorsFile:
    """A memory-mapped safetensors file."""

    def __init__(self, path: str):
        self.path = path
        self.tensors, self.metadata = read_header(path)
        size = os.path.getsize(path)
        self._mm = np.memmap(path, dtype=np.uint8, mode="r") if size else None

    def raw(self, name: str) -> np.ndarray:
        """Zero-copy flat view of a tensor's stored values (see _RAW_DTYPES)."""
        info = self.tensors[name]
        if info.numel == 0:
            return np.empty(0, dtype=_RAW_DTYPES[info.dtype])
        return self._mm[info.begin : info.end].view(_RAW_DTYPES[info.dtype])


# --------------------------------------------------------------------------- writing


def float32_to_bf16_bits(x: np.ndarray) -> np.ndarray:
    """Round float32 -> bfloat16 (round-to-nearest-even), returned as uint16 bits."""
    u = np.ascontiguousarray(x, dtype=np.float32).view(np.uint32)
    rounded = u + (np.uint32(0x7FFF) + ((u >> 16) & np.uint32(1)))
    out = (rounded >> 16).astype(np.uint16)
    nan = np.isnan(x)
    if nan.any():
        out[nan] = 0x7FC0
    return out


_NUMPY_TO_TAG = {np.dtype(v).str: k for k, v in _RAW_DTYPES.items() if k not in ("BF16", "F8_E4M3", "F8_E5M2", "U16", "U8")}
_NUMPY_TO_TAG[np.dtype("<u2").str] = "U16"
_NUMPY_TO_TAG[np.dtype("u1").str] = "U8"


def save_file(
    tensors: dict[str, np.ndarray],
    path: str,
    metadata: dict[str, str] | None = None,
    float_dtype: str | None = None,
) -> None:
    """Write tensors to a safetensors file.

    `float_dtype="BF16"` stores float arrays as bfloat16; `"F16"`/`"F32"` cast.
    A value may also be a `(raw_array, dtype_tag)` tuple to write pre-encoded
    bits (e.g. `(uint16_bits, "BF16")`).
    """
    header: dict[str, object] = {}
    if metadata:
        header["__metadata__"] = {str(k): str(v) for k, v in metadata.items()}
    blobs: list[bytes] = []
    offset = 0
    for name, value in tensors.items():
        if isinstance(value, tuple):
            arr, tag = value
            arr = np.ascontiguousarray(arr)
        else:
            arr = np.asarray(value)
            if arr.dtype.kind == "f" and float_dtype == "BF16":
                shape = arr.shape
                arr, tag = float32_to_bf16_bits(arr.astype(np.float32)).reshape(shape), "BF16"
            elif arr.dtype.kind == "f" and float_dtype in ("F16", "F32"):
                arr = arr.astype(_RAW_DTYPES[float_dtype])
                tag = float_dtype
            else:
                arr = arr.astype(arr.dtype.newbyteorder("<"))
                tag = _NUMPY_TO_TAG.get(arr.dtype.str)
                if tag is None:
                    raise SafetensorsError(f"{name}: unsupported numpy dtype {arr.dtype}")
            arr = np.ascontiguousarray(arr)
        data = arr.tobytes()
        header[name] = {"dtype": tag, "shape": list(arr.shape), "data_offsets": [offset, offset + len(data)]}
        blobs.append(data)
        offset += len(data)
    hjson = json.dumps(header, separators=(",", ":")).encode()
    hjson += b" " * (-len(hjson) % 8)  # spec: pad header to 8-byte alignment
    with open(path, "wb") as f:
        f.write(struct.pack("<Q", len(hjson)))
        f.write(hjson)
        for blob in blobs:
            f.write(blob)

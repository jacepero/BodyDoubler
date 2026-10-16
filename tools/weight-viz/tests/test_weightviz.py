"""Tests for weightviz. Run from tools/weight-viz:  python -m unittest discover -s tests"""

from __future__ import annotations

import base64
import json
import math
import os
import struct
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from weightviz import diff as diffmod  # noqa: E402
from weightviz.checkpoints import Checkpoint, discover  # noqa: E402
from weightviz.greenhouse import save_daily_checkpoint  # noqa: E402
from weightviz.layout import describe  # noqa: E402
from weightviz.safetensors_io import (  # noqa: E402
    SafetensorsError,
    SafetensorsFile,
    decode,
    float32_to_bf16_bits,
    read_header,
    save_file,
)


def bf16_round(x: np.ndarray) -> np.ndarray:
    """float32 values representable in bf16 (what a bf16 checkpoint stores)."""
    return decode(float32_to_bf16_bits(x.astype(np.float32)), "BF16").reshape(x.shape)


class TempDirTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = self._tmp.name

    def tearDown(self):
        self._tmp.cleanup()


# --------------------------------------------------------------------------- safetensors


class SafetensorsTests(TempDirTest):
    def test_roundtrip_dtypes(self):
        rng = np.random.default_rng(0)
        f32 = rng.normal(size=(5, 7)).astype(np.float32)
        tensors = {
            "f32": f32,
            "f16": f32.astype(np.float16),
            "i64": np.arange(6, dtype=np.int64).reshape(2, 3),
            "bf16": (float32_to_bf16_bits(f32), "BF16"),
            "scalar": np.float32(3.5).reshape(()),
            "empty": np.zeros((0, 4), np.float32),
        }
        path = os.path.join(self.tmp, "x.safetensors")
        save_file(tensors, path, metadata={"k": "v"})
        sf = SafetensorsFile(path)
        self.assertEqual(sf.metadata, {"k": "v"})
        self.assertEqual(sf.tensors["f32"].shape, (5, 7))
        np.testing.assert_array_equal(decode(sf.raw("f32"), "F32").reshape(5, 7), f32)
        np.testing.assert_array_equal(decode(sf.raw("f16"), "F16").reshape(5, 7), f32.astype(np.float16).astype(np.float32))
        np.testing.assert_array_equal(decode(sf.raw("i64"), "I64"), np.arange(6))
        np.testing.assert_array_equal(decode(sf.raw("bf16"), "BF16").reshape(5, 7), bf16_round(f32))
        np.testing.assert_allclose(decode(sf.raw("bf16"), "BF16").reshape(5, 7), f32, rtol=2**-8)
        self.assertEqual(float(decode(sf.raw("scalar"), "F32")[0]), 3.5)
        self.assertEqual(sf.raw("empty").size, 0)
        with open(path, "rb") as f:
            (n,) = struct.unpack("<Q", f.read(8))
        self.assertEqual(n % 8, 0, "header must be padded to 8 bytes")

    def test_bf16_round_to_nearest_even(self):
        x = np.array([1.0, 1 + 2**-8, 1 + 3 * 2**-8, -2.0, 1 + 2**-7 + 2**-9], np.float32)
        bits = float32_to_bf16_bits(x)
        self.assertEqual([hex(b) for b in bits], ["0x3f80", "0x3f80", "0x3f82", "0xc000", "0x3f81"])
        self.assertEqual(float32_to_bf16_bits(np.array([np.nan], np.float32))[0], 0x7FC0)

    def test_fp8_decoding(self):
        e4m3 = decode(np.array([0x38, 0x7E, 0x01, 0xB8, 0x00], np.uint8), "F8_E4M3")
        np.testing.assert_array_equal(e4m3, np.array([1.0, 448.0, 2.0**-9, -1.0, 0.0], np.float32))
        self.assertTrue(np.isnan(decode(np.array([0x7F], np.uint8), "F8_E4M3")[0]))
        e5m2 = decode(np.array([0x3C, 0xC0, 0x7B], np.uint8), "F8_E5M2")
        np.testing.assert_array_equal(e5m2, np.array([1.0, -2.0, 57344.0], np.float32))

    def test_truncated_file_is_rejected(self):
        path = os.path.join(self.tmp, "t.safetensors")
        save_file({"w": np.ones((64, 64), np.float32)}, path)
        size = os.path.getsize(path)
        with open(path, "r+b") as f:
            f.truncate(size - 100)
        with self.assertRaises(SafetensorsError):
            read_header(path)

    def test_interop_with_official_library(self):
        try:
            from safetensors.numpy import load_file, save_file as st_save
        except ImportError:
            self.skipTest("safetensors package not installed")
        rng = np.random.default_rng(1)
        a = rng.normal(size=(3, 4)).astype(np.float32)
        b = rng.normal(size=(8,)).astype(np.float16)
        ours = os.path.join(self.tmp, "ours.safetensors")
        save_file({"a": a, "b": b}, ours)
        loaded = load_file(ours)
        np.testing.assert_array_equal(loaded["a"], a)
        np.testing.assert_array_equal(loaded["b"], b)
        theirs = os.path.join(self.tmp, "theirs.safetensors")
        st_save({"a": a, "b": b}, theirs)
        sf = SafetensorsFile(theirs)
        np.testing.assert_array_equal(decode(sf.raw("a"), "F32").reshape(3, 4), a)
        np.testing.assert_array_equal(decode(sf.raw("b"), "F16"), b.astype(np.float32))


# --------------------------------------------------------------------------- layout


class LayoutTests(unittest.TestCase):
    def test_common_naming_schemes(self):
        cases = {
            "model.layers.12.self_attn.q_proj.weight": ("L12", "attn", "q_proj"),
            "model.layers.0.mlp.down_proj.weight": ("L0", "mlp", "down_proj"),
            "model.layers.3.post_attention_layernorm.weight": ("L3", "norm", "post_attention_layernorm"),
            "model.layers.3.mlp.experts.7.up_proj.weight": ("L3", "mlp", "experts.*.up_proj"),
            "transformer.h.2.attn.c_attn.bias": ("L2", "attn", "attn.c_attn (bias)"),
            "transformer.h.2.mlp.c_proj.weight": ("L2", "mlp", "mlp.c_proj"),
            "blk.5.ffn_down.weight": ("L5", "mlp", "ffn_down"),
            "model.embed_tokens.weight": ("embed", "embed", "embed_tokens"),
            "lm_head.weight": ("head", "embed", "lm_head"),
            "model.norm.weight": ("head", "norm", "norm"),
        }
        for name, (row, group, label) in cases.items():
            d = describe(name)
            self.assertEqual((d["row"], d["group"], d["module_label"]), (row, group, label), name)

    def test_column_order(self):
        names = ["down_proj", "q_proj", "input_layernorm", "v_proj", "gate_proj", "k_proj", "o_proj", "up_proj"]
        order = sorted(names, key=lambda m: describe(f"model.layers.0.{'mlp' if 'proj' in m and m[0] in 'gud' else 'self_attn'}.{m}.weight")["col_order"])
        self.assertEqual(order, ["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj", "input_layernorm"])


# --------------------------------------------------------------------------- diff math


def write_day(root, run, day, tensors, bf16=False):
    if bf16:
        tensors = {k: (float32_to_bf16_bits(v).reshape(v.shape), "BF16") for k, v in tensors.items()}
    return save_daily_checkpoint(tensors, root, run, day=day, meta={"note": day})


def load_pair(root, run):
    r = next(x for x in discover(root) if x.name == run)
    return Checkpoint(r.days[0]), Checkpoint(r.days[1])


class DiffTests(TempDirTest):
    def make_pair(self, bf16=False):
        rng = np.random.default_rng(2)
        a = {
            "model.layers.0.self_attn.q_proj.weight": rng.normal(0, 0.02, (37, 53)).astype(np.float32),
            "model.layers.0.input_layernorm.weight": np.ones(53, np.float32),
            "model.layers.1.mlp.up_proj.weight": rng.normal(0, 0.02, (64, 16)).astype(np.float32),
            "removed.weight": np.zeros(4, np.float32),
            "reshaped.weight": np.zeros((2, 3), np.float32),
        }
        b = {k: v.copy() for k, v in a.items() if k != "removed.weight"}
        b["model.layers.0.self_attn.q_proj.weight"] += rng.normal(0, 1e-3, (37, 53)).astype(np.float32)
        b["model.layers.0.input_layernorm.weight"][:10] += 0.01
        b["model.layers.1.mlp.up_proj.weight"] *= 1.5
        b["reshaped.weight"] = np.zeros((3, 2), np.float32)
        b["added.weight"] = np.ones(2, np.float32)
        write_day(self.tmp, "run", "2026-01-01", a, bf16)
        write_day(self.tmp, "run", "2026-01-02", b, bf16)
        if bf16:
            a = {k: bf16_round(v) for k, v in a.items()}
            b = {k: bf16_round(v) for k, v in b.items()}
        return a, b

    def check_pair_against_numpy(self, bf16):
        a, b = self.make_pair(bf16)
        ck_a, ck_b = load_pair(self.tmp, "run")
        res = diffmod.compute_pair(ck_a, ck_b, workers=2)
        t = res["tensors"]
        status = dict(zip(t["name"], t["status"]))
        self.assertEqual(status["removed.weight"], "removed")
        self.assertEqual(status["added.weight"], "added")
        self.assertEqual(status["reshaped.weight"], "reshaped")
        for i, name in enumerate(t["name"]):
            if status[name] != "ok":
                self.assertIsNone(t["sq_d"][i])
                continue
            A, B = a[name].astype(np.float64), b[name].astype(np.float64)
            D = B - A
            self.assertAlmostEqual(t["sq_a"][i], float((A * A).sum()), delta=1e-6 * float((A * A).sum()) + 1e-12)
            self.assertAlmostEqual(t["sq_d"][i], float((D * D).sum()), delta=1e-5 * float((D * D).sum()) + 1e-15)
            self.assertAlmostEqual(t["maxabs"][i], float(np.abs(D).max()), places=7)
            self.assertEqual(t["changed"][i], int(np.count_nonzero(D)))
            self.assertEqual(t["numel"][i], A.size)
            cos_direct = float((A * B).sum() / math.sqrt((A * A).sum() * (B * B).sum()))
            self.assertAlmostEqual(diffmod.cosine(t["sq_a"][i], t["sq_b"][i], t["sq_d"][i]), cos_direct, places=6)
        # layer rows come first in sorted order, unknown names last
        self.assertEqual(t["row"][0], "L0")

    def test_pair_stats_fp32(self):
        self.check_pair_against_numpy(bf16=False)

    def test_pair_stats_bf16(self):
        self.check_pair_against_numpy(bf16=True)

    def test_chunking_does_not_change_results(self):
        self.make_pair()
        ck_a, ck_b = load_pair(self.tmp, "run")
        whole = diffmod.compute_pair(ck_a, ck_b)
        name = "model.layers.0.self_attn.q_proj.weight"
        detail_whole = diffmod.tensor_detail(ck_a, ck_b, name, grid=8)
        with mock.patch.object(diffmod, "CHUNK", 97):  # forces many uneven chunks
            chunked = diffmod.compute_pair(ck_a, ck_b)
            detail_chunked = diffmod.tensor_detail(ck_a, ck_b, name, grid=8)
        for f in ("sq_a", "sq_d", "changed", "maxabs"):
            for x, y in zip(whole["tensors"][f], chunked["tensors"][f]):
                if x is None:
                    self.assertIsNone(y)
                else:
                    self.assertAlmostEqual(x, y, delta=1e-6 * abs(x) + 1e-15)
        for k in ("delta_mean", "delta_rms", "weight_rms"):
            np.testing.assert_allclose(
                np.frombuffer(base64.b64decode(detail_whole["grid"][k]), "<f4"),
                np.frombuffer(base64.b64decode(detail_chunked["grid"][k]), "<f4"),
                rtol=1e-5,
                atol=1e-12,
            )
        self.assertEqual(detail_whole["hist"]["counts"], detail_chunked["hist"]["counts"])

    def test_tensor_detail_grid_and_histogram(self):
        a, b = self.make_pair()
        ck_a, ck_b = load_pair(self.tmp, "run")
        name = "model.layers.0.self_attn.q_proj.weight"
        d = diffmod.tensor_detail(ck_a, ck_b, name, grid=8)
        g = d["grid"]
        br, bc = g["block"]
        self.assertEqual((g["rows"], g["cols"]), (math.ceil(37 / br), math.ceil(53 / bc)))
        D = (b[name] - a[name]).astype(np.float64)
        mean = np.frombuffer(base64.b64decode(g["delta_mean"]), "<f4").reshape(g["rows"], g["cols"])
        rms = np.frombuffer(base64.b64decode(g["delta_rms"]), "<f4").reshape(g["rows"], g["cols"])
        for i in range(g["rows"]):
            for j in range(g["cols"]):
                blk = D[i * br : (i + 1) * br, j * bc : (j + 1) * bc]
                self.assertAlmostEqual(float(mean[i, j]), blk.mean(), places=6)
                self.assertAlmostEqual(float(rms[i, j]), math.sqrt((blk**2).mean()), places=6)
        h = d["hist"]
        self.assertEqual(sum(h["counts"]) + h["below"] + h["above"] + h["zeros"], D.size)
        self.assertAlmostEqual(d["stats"]["rel"], float(np.linalg.norm(D) / np.linalg.norm(a[name])), places=5)

        one_d = diffmod.tensor_detail(ck_a, ck_b, "model.layers.0.input_layernorm.weight")
        self.assertEqual(one_d["grid"]["rows"], 1)
        self.assertIn("delta_min", one_d["grid"])
        self.assertEqual(one_d["hist"]["zeros"], 43)

    def test_spectrum_finds_low_rank_update(self):
        rng = np.random.default_rng(3)
        for rows, cols in ((300, 200), (1100, 1000)):  # exact path, randomized path
            with self.subTest(shape=(rows, cols)):
                w = rng.normal(0, 0.02, (rows, cols)).astype(np.float32)
                upd = (rng.normal(size=(rows, 3)) @ rng.normal(size=(3, cols)) * 1e-3).astype(np.float32)
                run = f"spec{rows}"
                write_day(self.tmp, run, "2026-01-01", {"m.weight": w})
                write_day(self.tmp, run, "2026-01-02", {"m.weight": w + upd})
                ck_a, ck_b = load_pair(self.tmp, run)
                s = diffmod.spectrum(ck_a, ck_b, "m.weight", k=8)
                self.assertEqual(s["method"], "exact" if rows * cols <= 1 << 20 else "randomized")
                self.assertGreater(sum(s["share"][:3]), 0.999)
                truth = np.linalg.svd(((w + upd) - w).astype(np.float64), compute_uv=False)[:3]
                np.testing.assert_allclose(s["values"][:3], truth, rtol=1e-3)


# --------------------------------------------------------------------------- discovery & writer


class CheckpointTests(TempDirTest):
    def test_discovery_layouts_and_incomplete_days(self):
        t = {"w": np.ones((4, 4), np.float32)}
        for day in ("step-10", "step-9", "step-100"):
            write_day(self.tmp, "runA", day, t)
        write_day(self.tmp, "runB", "2026-02-01", t)
        os.makedirs(os.path.join(self.tmp, "runB", ".2026-02-02.tmp-1"))  # in-progress write: ignored
        broken = os.path.join(self.tmp, "runB", "2026-02-03")
        os.makedirs(broken)
        with open(os.path.join(broken, "model.safetensors"), "wb") as f:
            f.write(b"\x10\x00\x00\x00\x00\x00\x00\x00{")
        runs = {r.name: r for r in discover(self.tmp)}
        self.assertEqual(sorted(runs), ["runA", "runB"])
        self.assertEqual([d.name for d in runs["runA"].days], ["step-9", "step-10", "step-100"])
        self.assertEqual([d.name for d in runs["runB"].days], ["2026-02-01", "2026-02-03"])
        self.assertIsNotNone(runs["runB"].days[1].error)
        self.assertEqual(len(runs["runB"].ready_days), 1)
        # A run folder can also be served directly.
        single = discover(os.path.join(self.tmp, "runA"))
        self.assertEqual([r.name for r in single], ["runA"])
        self.assertEqual(runs["runA"].days[0].meta["note"], "step-9")

    def test_sharded_writer_and_overwrite(self):
        rng = np.random.default_rng(4)
        tensors = {f"model.layers.{i}.mlp.up_proj.weight": rng.normal(size=(32, 32)).astype(np.float32) for i in range(5)}
        path = save_daily_checkpoint(tensors, self.tmp, "r", day="2026-03-01", meta={"loss": np.float32(2.5)}, max_shard_bytes=10_000)
        files = sorted(os.listdir(path))
        self.assertIn("model.safetensors.index.json", files)
        self.assertEqual(len([f for f in files if f.endswith(".safetensors")]), 3)  # 4 KB tensors, 10 KB shards
        with open(os.path.join(path, "meta.json")) as f:
            self.assertEqual(json.load(f)["loss"], 2.5)
        day = discover(self.tmp)[0].days[0]
        self.assertIsNone(day.error)
        ck = Checkpoint(day)
        for name, v in tensors.items():
            np.testing.assert_array_equal(decode(ck.raw(name), "F32").reshape(v.shape), v)
        # Re-saving the same day replaces it atomically and leaves no temp folders behind.
        save_daily_checkpoint({"x": np.zeros(3, np.float32)}, self.tmp, "r", day="2026-03-01")
        self.assertEqual(os.listdir(os.path.join(self.tmp, "r")), ["2026-03-01"])
        self.assertEqual(sorted(os.listdir(path)), ["meta.json", "model.safetensors"])
        with self.assertRaises(FileExistsError):
            save_daily_checkpoint({"x": np.zeros(3, np.float32)}, self.tmp, "r", day="2026-03-01", overwrite=False)
        with self.assertRaises(ValueError):
            save_daily_checkpoint({}, self.tmp, "r", day="../escape")


# --------------------------------------------------------------------------- HTTP API


class ServerTests(TempDirTest):
    def setUp(self):
        super().setUp()
        from http.server import ThreadingHTTPServer

        from weightviz.server import App, make_handler

        rng = np.random.default_rng(5)
        w = {"model.layers.0.self_attn.q_proj.weight": rng.normal(size=(16, 16)).astype(np.float32)}
        self.data = os.path.join(self.tmp, "data")
        write_day(self.data, "run", "2026-04-01", w)
        w2 = {k: v + 0.01 for k, v in w.items()}
        write_day(self.data, "run", "2026-04-02", w2)
        self.app = App(self.data, os.path.join(self.tmp, "cache"))
        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(self.app))
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()
        self.base = f"http://127.0.0.1:{self.httpd.server_address[1]}"

    def tearDown(self):
        self.httpd.shutdown()
        self.httpd.server_close()
        super().tearDown()

    def get(self, path):
        try:
            with urllib.request.urlopen(self.base + path, timeout=10) as r:
                return r.status, r.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()

    def get_json(self, path):
        status, body = self.get(path)
        return status, json.loads(body)

    def test_end_to_end(self):
        status, runs = self.get_json("/api/runs")
        self.assertEqual(status, 200)
        self.assertEqual(runs["runs"][0]["name"], "run")
        q = "run=run&a=2026-04-01&b=2026-04-02"
        for _ in range(100):
            status, pair = self.get_json(f"/api/pair?{q}")
            if pair["state"] == "done":
                break
            time.sleep(0.05)
        self.assertEqual(pair["state"], "done")
        self.assertEqual(pair["result"]["tensors"]["changed"], [256])
        status, tl = self.get_json("/api/timeline?run=run")
        self.assertEqual(tl["pairs"][0]["state"], "done")
        name = "model.layers.0.self_attn.q_proj.weight"
        status, detail = self.get_json(f"/api/tensor?{q}&name={name}")
        self.assertEqual(status, 200)
        self.assertAlmostEqual(detail["stats"]["maxabs"], 0.01, places=5)
        status, spec = self.get_json(f"/api/spectrum?{q}&name={name}")
        self.assertEqual(status, 200)
        self.assertGreater(spec["share"][0], 0.99)  # adding a constant is a rank-1 update
        # Cached on disk: a fresh App answers immediately.
        from weightviz.server import App

        app2 = App(self.data, os.path.join(self.tmp, "cache"))
        a, b = app2.run("run").days
        self.assertEqual(app2.pairs.request(a, b)["state"], "done")

    def test_errors_and_static(self):
        self.assertEqual(self.get_json("/api/pair?run=nope&a=x&b=y")[0], 404)
        self.assertEqual(self.get_json("/api/pair?run=run&a=2026-04-01&b=nope")[0], 404)
        self.assertEqual(self.get_json("/api/tensor?run=run&a=2026-04-01&b=2026-04-02&name=missing")[0], 404)
        status, body = self.get("/")
        self.assertEqual(status, 200)
        self.assertIn(b"Greenhouse", body)
        self.assertEqual(self.get("/static/app.js")[0], 200)
        self.assertEqual(self.get("/static/../server.py")[0], 404)
        self.assertEqual(self.get("/static/%2e%2e/server.py")[0], 404)


if __name__ == "__main__":
    unittest.main()

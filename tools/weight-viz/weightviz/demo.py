"""Generate a synthetic Greenhouse run so the visualizer can be tried without real checkpoints.

A small Qwen-shaped transformer (~9M params) "trains" for a week. fp32 master
weights are updated each day and saved as bf16, as a real run would. Planted
events, so there's something to find:

  * learning rate decays day over day; deeper layers move more early on
  * attention q/k updates are low-rank (few directions carry the energy), MLP updates are diffuse
  * only the first 1024 embedding rows ("frequent tokens") are trained
  * day 4: embeddings frozen (exactly zero change)
  * day 6: a data-mix change hits layer 6's MLP hard (and bumps the loss)
"""

from __future__ import annotations

import datetime as dt

import numpy as np

from .greenhouse import save_daily_checkpoint

VOCAB, D, LAYERS, KV, FFN, HEAD_DIM = 4096, 256, 10, 128, 704, 32
SCHEDULE = [1.0, 0.85, 0.7, 0.55, 0.4, 0.3, 0.2]
LOSS = [3.91, 3.24, 2.93, 2.74, 2.62, 2.71, 2.55, 2.51]


def init_weights(rng: np.random.Generator) -> dict[str, np.ndarray]:
    w = {"model.embed_tokens.weight": rng.normal(0, 0.02, (VOCAB, D))}
    for i in range(LAYERS):
        p = f"model.layers.{i}."
        w[p + "input_layernorm.weight"] = np.ones(D)
        w[p + "self_attn.q_proj.weight"] = rng.normal(0, 0.02, (D, D))
        w[p + "self_attn.k_proj.weight"] = rng.normal(0, 0.02, (KV, D))
        w[p + "self_attn.v_proj.weight"] = rng.normal(0, 0.02, (KV, D))
        w[p + "self_attn.o_proj.weight"] = rng.normal(0, 0.02, (D, D))
        w[p + "self_attn.q_norm.weight"] = np.ones(HEAD_DIM)
        w[p + "self_attn.k_norm.weight"] = np.ones(HEAD_DIM)
        w[p + "post_attention_layernorm.weight"] = np.ones(D)
        w[p + "mlp.gate_proj.weight"] = rng.normal(0, 0.02, (FFN, D))
        w[p + "mlp.up_proj.weight"] = rng.normal(0, 0.02, (FFN, D))
        w[p + "mlp.down_proj.weight"] = rng.normal(0, 0.02 / np.sqrt(2 * LAYERS), (D, FFN))
    w["model.norm.weight"] = np.ones(D)
    w["lm_head.weight"] = rng.normal(0, 0.02, (VOCAB, D))
    return w


def _update(rng, shape, rank: int | None, noise: float) -> np.ndarray:
    """Unit-RMS random update: low-rank part (if rank) + isotropic noise."""
    out = rng.normal(0, noise, shape)
    if rank:
        out += rng.normal(0, 1, (shape[0], rank)) @ rng.normal(0, 1, (rank, shape[1])) / np.sqrt(rank)
    return out / np.sqrt(np.mean(out * out))


def train_one_day(w: dict[str, np.ndarray], day: int, rng: np.random.Generator, gains: dict[str, float]) -> None:
    lr = 0.012 * SCHEDULE[day - 1]
    for name, val in w.items():
        depth = gains[name]  # per-tensor effective learning rate (Adam-like), fixed across days
        if ".layers." in name:
            layer = int(name.split(".")[2])
            depth *= 0.4 + 1.2 * layer / (LAYERS - 1) if day <= 3 else 1.0
        if val.ndim == 1:  # norm gains: small diffuse drift around 1
            val += lr * 0.4 * depth * rng.normal(0, 1, val.shape)
            continue
        scale = float(np.sqrt(np.mean(val * val))) * lr * depth  # relative to the tensor's own size
        if "embed_tokens" in name:
            if day == 4:
                continue  # frozen
            upd = np.zeros_like(val)
            upd[:1024] = _update(rng, (1024, D), None, 1.0) * 2.0
            val += scale * upd
        elif "q_proj" in name or "k_proj" in name:
            val += scale * _update(rng, val.shape, rank=4, noise=0.15)
        elif "v_proj" in name or "o_proj" in name:
            val += scale * _update(rng, val.shape, rank=16, noise=0.6)
        else:
            boost = 6.0 if day == 6 and ".layers.6.mlp." in name else 1.0
            val += scale * boost * _update(rng, val.shape, rank=None, noise=1.0)


def make_demo(root: str, run: str = "demo-qwen-9m", days: int = 8, end: dt.date | None = None) -> str:
    rng = np.random.default_rng(7)
    end = end or dt.date.today()
    start = end - dt.timedelta(days=days - 1)
    w = init_weights(rng)
    gains = {n: float(np.exp(rng.normal(0, 0.3))) for n in w}
    notes = {1: "SFT mix v1 begins", 4: "embeddings frozen", 6: "data mix v2 (code-heavy)"}
    for i in range(days):
        if i:
            train_one_day(w, i, rng, gains)
        day = (start + dt.timedelta(days=i)).isoformat()
        meta = {"step": 1500 * i, "tokens": 1500 * i * 2**20, "loss": LOSS[min(i, len(LOSS) - 1)]}
        if i:
            meta["lr"] = round(3e-5 * SCHEDULE[min(i, len(SCHEDULE)) - 1], 8)
        if i in notes:
            meta["notes"] = notes[i]
        tensors = {n: (v.astype(np.float32)) for n, v in w.items()}
        save_daily_checkpoint(_bf16(tensors), root, run, day=day, meta=meta)
        print(f"  wrote {run}/{day}")
    return f"{root}/{run}"


def _bf16(tensors: dict[str, np.ndarray]) -> dict:
    from .safetensors_io import float32_to_bf16_bits

    return {n: (float32_to_bf16_bits(v).reshape(v.shape), "BF16") for n, v in tensors.items()}

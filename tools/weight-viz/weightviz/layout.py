"""Map tensor names onto a (layer row, module column) grid.

Handles the common naming schemes: Hugging Face Llama/Qwen/Mistral
(`model.layers.3.self_attn.q_proj.weight`), GPT-2 (`transformer.h.3.attn.c_attn.weight`),
BERT (`encoder.layer.3.attention.self.query.weight`) and GGUF (`blk.3.attn_q.weight`).
Anything unrecognised still gets a row and column, just less neatly ordered.
"""

from __future__ import annotations

import re

_LAYER_RE = re.compile(r"(?:^|\.)(?:layers|layer|h|blocks|block|blk)\.(\d+)\.(.+)$")
_EMBED_RE = re.compile(r"embed|wte|wpe|tok_emb|token_embd|embeddings")
# Display-only shortening; "mlp." is dropped only for Llama-style names so GPT-2's
# attn.c_proj and mlp.c_proj stay distinguishable.
_PREFIX_RE = re.compile(r"^(?:model\.|transformer\.|self_attn\.|feed_forward\.|mlp\.(?=(?:gate|up|down)_proj|experts|gate|router))+")
_INNER_INDEX_RE = re.compile(r"\.(\d+)(?=\.|$)")

GROUP_ORDER = {"embed": 0, "attn": 1, "mlp": 2, "norm": 3, "other": 4}

# (group, pattern, rank) - first match wins within a group.
_RANKS = {
    "attn": [
        (r"qkv|c_attn|query_key_value", 0),
        (r"q_proj|query|\bwq\b|attn_q\b|\.q$", 1),
        (r"k_proj|\bkey\b|\bwk\b|attn_k\b|\.k$", 2),
        (r"v_proj|value|\bwv\b|attn_v\b|\.v$", 3),
        (r"o_proj|out_proj|\bwo\b|attn_output|dense|c_proj", 4),
    ],
    "mlp": [
        (r"router|\.gate$|gate\.weight", 0),
        (r"gate_proj|\bw1\b|ffn_gate", 1),
        (r"up_proj|\bw3\b|fc1|c_fc|ffn_up|intermediate", 2),
        (r"down_proj|\bw2\b|fc2|c_proj|ffn_down|output", 3),
    ],
    "norm": [
        (r"input_layernorm|ln_1|attn_norm|attention_norm", 0),
        (r"q_norm|k_norm", 1),
        (r"post_attention_layernorm|ln_2|ffn_norm", 2),
    ],
}


def _group(module: str) -> str:
    m = module.lower()
    if re.search(r"norm|(?:^|[._])ln(?:_|\d|$)", m):
        return "norm"
    if re.search(r"attn|attention|q_proj|k_proj|v_proj|o_proj|query|key|value|\bw[qkvo]\b|qkv", m):
        return "attn"
    if re.search(r"mlp|ffn|feed_forward|gate|up_proj|down_proj|fc\d|\bw[123]\b|expert|c_fc|router|intermediate|^output", m):
        return "mlp"
    return "other"


def _rank(group: str, module: str) -> int:
    for pattern, rank in _RANKS.get(group, []):
        if re.search(pattern, module):
            return rank
    return 50


def describe(name: str) -> dict:
    """Row/column placement for one tensor name."""
    base, kind = name, ""
    for suffix in (".weight", ".bias", ".scale"):
        if name.endswith(suffix):
            base, kind = name[: -len(suffix)], suffix[1:]
            break
    m = _LAYER_RE.search(base + ".")
    if m:
        layer = int(m.group(1))
        module = m.group(2).rstrip(".")
        row, row_order, row_label = f"L{layer}", layer, f"L{layer}"
        group = _group(module)
    else:
        layer = None
        module = base
        if _EMBED_RE.search(base.lower()):
            row, row_order, row_label, group = "embed", -1, "embed", "embed"
        else:
            row, row_order, row_label = "head", 1_000_000, "head"
            group = "norm" if _group(base) == "norm" else "embed" if re.search(r"lm_head|output", base) else _group(base)
    # Experts / heads stored as separate tensors aggregate into one column.
    module = _INNER_INDEX_RE.sub(".*", module)
    if kind == "bias":
        module += " (bias)"
    label = _PREFIX_RE.sub("", module) or module
    col_order = (GROUP_ORDER.get(group, 9) * 100 + _rank(group, module)) * 2 + (1 if kind == "bias" else 0)
    return {
        "layer": layer,
        "row": row,
        "row_order": row_order,
        "row_label": row_label,
        "module": module,
        "module_label": label,
        "group": group,
        "col_order": col_order,
    }

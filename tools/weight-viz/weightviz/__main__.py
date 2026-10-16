"""weightviz command line.

    python -m weightviz serve /data/greenhouse/checkpoints     # open http://127.0.0.1:8765
    python -m weightviz demo ./demo-checkpoints                 # synthetic run to try it on
    python -m weightviz diff RUN_DIR DAY_A DAY_B [--top 20]     # quick text summary
"""

from __future__ import annotations

import argparse
import math
import os
import sys


def _cmd_serve(args) -> None:
    from .server import serve

    if not os.path.isdir(args.root):
        sys.exit(f"not a directory: {args.root}")
    serve(args.root, args.host, args.port, args.cache_dir, args.workers)


def _cmd_demo(args) -> None:
    from .demo import make_demo

    path = make_demo(args.root, run=args.run, days=args.days)
    print(f"done. now run:  python -m weightviz serve {os.path.dirname(path) or '.'}")


def _cmd_diff(args) -> None:
    from .checkpoints import Checkpoint, discover
    from .diff import compute_pair

    runs = {r.path: r for r in discover(args.run_dir)}
    run = runs.get(os.path.abspath(args.run_dir)) or next(iter(runs.values()), None)
    if run is None:
        sys.exit(f"no checkpoints found under {args.run_dir}")
    a, b = run.day(args.day_a), run.day(args.day_b)
    for d in (a, b):
        if d.error:
            sys.exit(f"{d.name}: {d.error}")

    shown = [-1]

    def progress(p):
        pct = int(p * 100)
        if pct != shown[0] and sys.stderr.isatty():
            shown[0] = pct
            print(f"\r  diffing {a.name} -> {b.name}: {pct:3d}%", end="", file=sys.stderr)

    res = compute_pair(Checkpoint(a), Checkpoint(b), progress=progress)
    print(file=sys.stderr)
    t = res["tensors"]
    rows = []
    tot_a = tot_d = 0.0
    for i, name in enumerate(t["name"]):
        if t["status"][i] != "ok":
            print(f"  {t['status'][i]:>8}  {name}")
            continue
        tot_a += t["sq_a"][i]
        tot_d += t["sq_d"][i]
        rel = math.sqrt(t["sq_d"][i] / t["sq_a"][i]) if t["sq_a"][i] else float("nan")
        rows.append((rel, name, t["changed"][i] / max(t["numel"][i], 1)))
    print(f"whole model: ||dW||/||W|| = {math.sqrt(tot_d / tot_a) if tot_a else float('nan'):.3e}  ({res['elapsed']}s)")
    print(f"top {args.top} tensors by relative change:")
    for rel, name, changed in sorted(rows, key=lambda r: -r[0] if r[0] == r[0] else 0)[: args.top]:
        print(f"  {rel:.3e}  changed {changed:6.1%}  {name}")


def main(argv=None) -> None:
    p = argparse.ArgumentParser(prog="weightviz", description="Day-over-day LLM weight diff visualizer for Greenhouse.")
    sub = p.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("serve", help="run the web UI")
    s.add_argument("root", help="checkpoint root (folder of runs, or a single run folder)")
    s.add_argument("--host", default="127.0.0.1", help="bind address (default: localhost only)")
    s.add_argument("--port", type=int, default=8765)
    s.add_argument("--cache-dir", default=os.path.join(os.path.expanduser("~"), ".cache", "weightviz"))
    s.add_argument("--workers", type=int, default=None, help="threads per diff (default: min(8, cpus))")
    s.set_defaults(func=_cmd_serve)

    d = sub.add_parser("demo", help="write a synthetic week-long run")
    d.add_argument("root", nargs="?", default="demo-checkpoints")
    d.add_argument("--run", default="demo-qwen-9m")
    d.add_argument("--days", type=int, default=8)
    d.set_defaults(func=_cmd_demo)

    f = sub.add_parser("diff", help="print a text summary of one day pair")
    f.add_argument("run_dir")
    f.add_argument("day_a")
    f.add_argument("day_b")
    f.add_argument("--top", type=int, default=20)
    f.set_defaults(func=_cmd_diff)

    args = p.parse_args(argv)
    args.func(args)


if __name__ == "__main__":
    main()

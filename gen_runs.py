#!/usr/bin/env python3
"""Bake model runs: ./gen_runs.py post.html [--hole N | --all] [--model M] [--override]

Each run lives at posts/runs/<slug>/h<i>/<md5>__<model>.html, where <md5>
is the hash of the prompt (everything above the marker, markers stripped).

The model is controlled by the file: a !!!openai/gpt-6-luna!!! block names
both the hole and the model to bake. Legacy <!!!AI!!!> blocks still work
but need --model. Re-running with unchanged input is a cache hit (skipped);
changed input bakes fresh and prunes the stale file for that model.
"""
import argparse
import hashlib
import json
import os
import re
import sys
from pathlib import Path

API = "https://openrouter.ai/api/v1/chat/completions"
MARKER = re.compile(r'!!!(.+?)!!!')
HASHED = re.compile(r'^([0-9a-f]{32})__(.+)$')


def key() -> str:
    if k := os.environ.get("OPENROUTER_KEY"):
        return k
    env = Path(__file__).with_name(".env")
    if env.exists():
        for line in env.read_text().splitlines():
            k, _, v = line.partition("=")
            if k.strip() == "OPENROUTER_KEY":
                return v.strip().strip("\"'")
    sys.exit("set OPENROUTER_KEY or add it to .env")


def run(k: str, model: str, prompt: str) -> str:
    import urllib.request
    body = json.dumps({"model": model, "messages": [{"role": "user", "content": prompt}]}).encode()
    req = urllib.request.Request(API, data=body, headers={
        "Authorization": f"Bearer {k}", "Content-Type": "application/json",
        "HTTP-Referer": "https://tommy.isnt.online", "X-Title": "blog jukebox bake"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.load(r)["choices"][0]["message"]["content"]


def holes(post: str):
    """Yield (marker_no, model_or_None, context) per !!!...!!! block, in order.

    A block holding exactly 'AI' is a legacy hole (model must come from
    --model); anything else names its model. Context is the text above the
    block with all marker blocks stripped out.
    """
    out = []
    for n, m in enumerate(MARKER.finditer(post), start=1):
        inner = m.group(1).strip()
        ctx = MARKER.sub("", post[:m.start()]).strip()
        out.append((n, None if inner == "AI" else inner, ctx))
    return out


def existing_runs(outdir: Path):
    """Map model_enc -> list of (md5_or_None, path) for baked files."""
    found = {}
    if not outdir.is_dir():
        return found
    for path in sorted(outdir.glob("*.html")):
        stem = path.stem
        m = HASHED.match(stem)
        md5, model_enc = (m.group(1), m.group(2)) if m else (None, stem)
        found.setdefault(model_enc, []).append((md5, path))
    return found


def bake_one(slug, hole_no, model, ctx, outdir, override, k):
    model_enc = model.replace("/", "__")
    digest = hashlib.md5(ctx.encode()).hexdigest()
    if not ctx:
        sys.exit(f"h{hole_no}: empty context")
    current = existing_runs(outdir).get(model_enc, [])
    if not override and any(md5 == digest for md5, _ in current):
        print(f"h{hole_no}/{model}: cached ({digest[:12]}...), skipping")
        return
    print(f"h{hole_no}/{model}: baking ({len(ctx)} chars) ...", flush=True)
    text = run(k, model, ctx).strip() + "\n"
    path = outdir / f"{digest}__{model_enc}.html"
    path.write_text(text)
    for _, stale in current:
        if stale != path:
            stale.unlink()
    print(f"h{hole_no}/{model}: wrote {path}")
    print(text)


def main() -> None:
    ap = argparse.ArgumentParser(description="Bake AI runs for a blog post; model comes from !!!model!!! blocks in the file.")
    ap.add_argument("post", help="post .html file")
    ap.add_argument("--hole", type=int, default=1, help="which !!!...!!! block to bake (1-based)")
    ap.add_argument("--all", action="store_true", help="bake every !!!...!!! block in the file")
    ap.add_argument("--model", help="model for legacy <!!!AI!!!> blocks (ignored otherwise)")
    ap.add_argument("--override", action="store_true", help="rebake even on cache hit")
    args = ap.parse_args()

    if args.all and args.hole != 1:
        ap.error("--hole and --all are mutually exclusive")

    post = Path(args.post).read_text()
    slug = Path(args.post).stem
    found = holes(post)
    if not found:
        sys.exit(f"{slug}: no !!!...!!! blocks")
    if args.all:
        selected = found
    else:
        if not 1 <= args.hole <= len(found):
            sys.exit(f"hole h{args.hole} missing (found {len(found)})")
        selected = [found[args.hole - 1]]

    k = key()
    for n, model, ctx in selected:
        if model is None:
            if not args.model:
                sys.exit(f"h{n}: legacy <!!!AI!!!> block needs --model (or switch it to !!!model!!!)")
            model = args.model
        elif args.model:
            sys.exit(f"h{n}: model comes from the file ({model}); --model is only for legacy <!!!AI!!!>")
        outdir = Path("posts/runs") / slug / f"h{n}"
        outdir.mkdir(parents=True, exist_ok=True)
        bake_one(slug, n, model, ctx, outdir, args.override, k)


if __name__ == "__main__":
    main()

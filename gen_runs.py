#!/usr/bin/env python3
"""Bake model runs: ./gen_runs.py post.html [--hole N | --all] [--override]

Each run lives at posts/runs/<slug>/h<i>/<md5>__<model>.html, where <md5>
is the hash of the prompt. The prompt is everything above the marker --
narrative plus all baked AI outputs above it, so every model sees what
the earlier models said. Re-running with unchanged input is a cache hit
(skipped); changed input bakes fresh and prunes only that model's stale
file. A hole can hold a comparison drawer of several models.
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
    """Yield (marker_no, model, marker_start) per !!!model!!! block.

    The block names its model (e.g. !!!openai/gpt-6-luna!!!), which must
    look like an openrouter id (org/name).
    """
    out = []
    for n, m in enumerate(MARKER.finditer(post), start=1):
        model = m.group(1).strip()
        if "/" not in model:
            sys.exit(f"h{n}: !!!{model}!!! doesn't look like an openrouter model (want org/name)")
        out.append((n, model, m.start()))
    return out


def build_ctx(post: str, start: int, slug: str, hole_no: int) -> str:
    """Prompt for a hole: narrative above the marker plus every baked
    AI output above it, so each model sees what the earlier ones said."""
    ctx = MARKER.sub("", post[:start]).strip()
    chunks = [ctx] if ctx else []
    for i in range(1, hole_no):
        idir = Path("posts/runs") / slug / f"h{i}"
        if not idir.is_dir():
            continue
        for path in sorted(idir.glob("*.html")):
            m = HASHED.match(path.stem)
            model = (m.group(2) if m else path.stem).replace("__", "/")
            text = path.read_text().strip()
            if text:
                chunks.append(f"--- baked h{i} ({model}) ---\n{text}")
    return "\n\n".join(chunks).strip()


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


def prune_stale(outdir, keep):
    """One run per hole: drop every other baked file in the hole dir."""
    if not outdir.is_dir():
        return
    for path in sorted(outdir.glob("*.html")):
        if path != keep:
            path.unlink()


def bake_one(slug, hole_no, model, ctx, outdir, override, k):
    model_enc = model.replace("/", "__")
    if not ctx:
        sys.exit(f"h{hole_no}: empty context")
    prompt = f"you are {model}\n\n{ctx}"
    digest = hashlib.md5(prompt.encode()).hexdigest()
    current = existing_runs(outdir).get(model_enc, [])
    hit = next((p for md5, p in current if md5 == digest), None)
    if hit is not None and not override:
        print(f"h{hole_no}/{model}: cached ({digest[:12]}...), skipping")
        keep = hit
    else:
        outdir.mkdir(parents=True, exist_ok=True)
        print(f"h{hole_no}/{model}: baking ({len(prompt)} chars) ...", flush=True)
        text = run(k, model, prompt).strip() + "\n"
        keep = outdir / f"{digest}__{model_enc}.html"
        keep.write_text(text)
        print(f"h{hole_no}/{model}: wrote {keep}")
        print(text)
    prune_stale(outdir, keep)


def main() -> None:
    ap = argparse.ArgumentParser(description="Bake AI runs for a blog post; model comes from !!!model!!! blocks in the file.")
    ap.add_argument("post", help="post .html file")
    ap.add_argument("--hole", type=int, default=None, help="bake only this !!!model!!! block (1-based); default bakes every block")
    ap.add_argument("--all", action="store_true", help="bake every !!!model!!! block in the file (the default)")
    ap.add_argument("--override", action="store_true", help="rebake even on cache hit")
    args = ap.parse_args()

    if args.hole is not None and args.all:
        ap.error("--hole and --all are mutually exclusive")

    post = Path(args.post).read_text()
    slug = Path(args.post).stem
    found = holes(post)
    if not found:
        sys.exit(f"{slug}: no !!!model!!! blocks")
    if args.hole is not None:
        if not 1 <= args.hole <= len(found):
            sys.exit(f"hole h{args.hole} missing (found {len(found)})")
        selected = [found[args.hole - 1]]
    else:
        selected = found

    k = key()
    for n, model, start in selected:
        ctx = build_ctx(post, start, slug, n)
        outdir = Path("posts/runs") / slug / f"h{n}"
        bake_one(slug, n, model, ctx, outdir, args.override, k)


if __name__ == "__main__":
    main()

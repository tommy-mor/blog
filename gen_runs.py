#!/usr/bin/env python3
"""Bake one model run: ./gen_runs.py <post.html> <openrouter/model> [hole=1] — prompt is everything above <!!!AI!!!>."""
import json, os, sys, urllib.request
from pathlib import Path

API = "https://openrouter.ai/api/v1/chat/completions"
HOLE = "<!!!AI!!!>"

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
    body = json.dumps({"model": model, "messages": [{"role": "user", "content": prompt}]}).encode()
    req = urllib.request.Request(API, data=body, headers={
        "Authorization": f"Bearer {k}", "Content-Type": "application/json",
        "HTTP-Referer": "https://tommy.isnt.online", "X-Title": "blog jukebox bake"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.load(r)["choices"][0]["message"]["content"]

def main() -> None:
    if len(sys.argv) not in (3, 4):
        sys.exit(f"usage: {sys.argv[0]} <post.html> <openrouter/model> [hole-index=1]")
    post = Path(sys.argv[1]).read_text()
    slug = Path(sys.argv[1]).stem
    model = sys.argv[2]
    parts = post.split(HOLE)
    if len(parts) < 2:
        sys.exit(f"{slug}: no {HOLE} holes")
    i = int(sys.argv[3]) if len(sys.argv) == 4 else 1
    if not 1 <= i < len(parts):
        sys.exit(f"hole h{i} missing (found {len(parts) - 1})")
    k = key()
    ctx = "\n".join(parts[:i]).strip()
    if not ctx:
        sys.exit(f"h{i}: empty context")
    outdir = Path("posts/runs") / slug / f"h{i}"
    outdir.mkdir(parents=True, exist_ok=True)
    path = outdir / f"{model.replace('/', '-')}.md"
    if path.exists():
        sys.exit(f"h{i}/{model}: exists, refusing to overwrite")
    print(f"h{i}/{model}: baking ({len(ctx)} chars) ...", flush=True)
    path.write_text(run(k, model, ctx).strip() + "\n")
    print(f"h{i}/{model}: wrote {path}")

if __name__ == "__main__":
    main()

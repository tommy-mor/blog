#!/usr/bin/env python3
"""Bake model runs for the blog jukebox: the AI reads all above the hole.

    OPENROUTER_KEY=... ./gen_runs.py <slug> [hole]

Holes are positional: h1 is the 1st <!!!AI!!!> in posts/<slug>.html.
With no hole arg, bakes every hole that has no runs yet. For each hole:
the prompt is everything above the marker (tags stripped), each model
in MODELS responds into posts/runs/<slug>/h<i>/<model>.md.

Curated and scrubbed by Master before anything lands here. The web side
never generates, never bills, never sees a key — pure playback.
"""

import json
import os
import re
import sys
import urllib.request

MODELS = [
    # "moonshotai/kimi-k3",
    # "anthropic/claude-opus-4-6",
    # "google/gemini-3-pro-preview",
]

API = "https://openrouter.ai/api/v1/chat/completions"
HOLE = "<!!!AI!!!>"


def strip_tags(s: str) -> str:
    return re.sub(r"<[^>]*>", "", s).strip()


def context_above(body: str, index: int) -> str:
    """Everything above the index-th hole (1-based), tags stripped."""
    parts = body.split(HOLE)
    assert 1 <= index <= len(parts) - 1, f"hole h{index} missing"
    return strip_tags("\n".join(parts[:index]))


def run_model(key: str, model: str, prompt: str) -> str:
    body = json.dumps(
        {"model": model, "messages": [{"role": "user", "content": prompt}]}
    ).encode()
    req = urllib.request.Request(
        API,
        data=body,
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
            "HTTP-Referer": "https://tommy.isnt.online",
            "X-Title": "blog jukebox bake",
        },
    )
    with urllib.request.urlopen(req, timeout=300) as r:
        out = json.load(r)
    return out["choices"][0]["message"]["content"]


def holes_in(body: str) -> int:
    return body.count(HOLE)


def main() -> None:
    if len(sys.argv) not in (2, 3):
        sys.exit(f"usage: {sys.argv[0]} <slug> [hole-index]")
    slug = sys.argv[1]
    post = None
    for f in os.listdir("posts"):
        if f.endswith(".html") and f.startswith("20"):
            text = open(os.path.join("posts", f)).read()
            # slug match: posts loader slugifies the title line
            title = text.split("\n", 1)[0]
            import re as _re

            bits = [
                b
                for b in _re.sub(r"[^a-z0-9]+", "-", title.lower()).split("-")
                if b
            ]
            if "-".join(bits) == slug:
                post = text
                break
    if post is None:
        sys.exit(f"no post with slug {slug}")
    n = holes_in(post)
    if n == 0:
        sys.exit(f"{slug}: no <!!!AI!!!> holes")
    want = [int(sys.argv[2])] if len(sys.argv) == 3 else list(range(1, n + 1))
    key = os.environ.get("OPENROUTER_KEY", "")
    if not key:
        sys.exit("set OPENROUTER_KEY")
    if not MODELS:
        sys.exit("fill MODELS in gen_runs.py first")
    for i in want:
        ctx = context_above(post, i)
        if not ctx.strip():
            print(f"h{i}: empty context, skipping")
            continue
        outdir = os.path.join("posts", "runs", slug, f"h{i}")
        os.makedirs(outdir, exist_ok=True)
        for model in MODELS:
            safe = model.replace("/", "-")
            path = os.path.join(outdir, f"{safe}.md")
            if os.path.exists(path):
                print(f"h{i}/{model}: exists, skipping")
                continue
            print(f"h{i}/{model}: baking ({len(ctx)} chars context) ...", flush=True)
            text = run_model(key, model, ctx)
            with open(path, "w") as f:
                f.write(text.strip() + "\n")
            print(f"h{i}/{model}: wrote {path}")


if __name__ == "__main__":
    main()

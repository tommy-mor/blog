#!/usr/bin/env python3
"""Bake one AI turn per !!!org/model!!! marker in a blog post.

    ./bake.py post.html [--hole N] [--override] [--dry-run]

Each marker names its model. The prompt is the prose above the marker
interleaved with every earlier baked turn -- exactly the order the reader
encounters them (order invariant tested in bake_test.py). Code blocks
enter only via the post's own !!!read:...!!! markers, never hardcoded.
Runs live at posts/runs/<post>/h<N>/<md5>__<model>.html; the
md5 covers the whole prompt, so an edited upstream turn changes every
downstream digest, and a plain run rebakes exactly the stale tail.
"""
import argparse, hashlib, json, os, re, sys, urllib.request, urllib.error
from pathlib import Path

API = "https://openrouter.ai/api/v1/chat/completions"
MARKER = re.compile(r"!!!(.+?)!!!")
MODEL = re.compile(r"[\w.\-]+/[\w.\-]+")
HASHED = re.compile(r"^[0-9a-f]{32}__")


def key() -> str:
    if k := os.environ.get("OPENROUTER_KEY"):
        return k
    env = Path(__file__).with_name(".env")
    for line in (env.read_text().splitlines() if env.exists() else []):
        a, _, b = line.partition("=")
        if a.strip() == "OPENROUTER_KEY":
            return b.strip().strip("\"'")
    sys.exit("set OPENROUTER_KEY or add it to .env")


def call(k: str, model: str, prompt: str) -> str:
    """The LLM call: one user message, thinking on, content back."""
    req = urllib.request.Request(API, headers={
        "Authorization": f"Bearer {k}", "Content-Type": "application/json",
        "HTTP-Referer": "https://tommy.isnt.online", "X-Title": "blog jukebox bake"},
        data=json.dumps({"model": model, "reasoning": {"effort": "high"},
                         "messages": [{"role": "user", "content": prompt}]}).encode())
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            return json.load(r)["choices"][0]["message"]["content"]
    except urllib.error.HTTPError as e:
        sys.exit(f"openrouter {e.code} for {model}: {e.read().decode()[:300]}")


def build_ctx(post: str, start: int, slug: str) -> str:
    """The context buildup: prose and baked turns interleaved at marker
    positions -- the reader's exact order. Code reaches the prompt only
    when the post asks for it, via !!!read:...!!! markers."""
    me = Path(__file__).name
    chunks = []
    narr = MARKER.split(post[:start])[::2]  # odd slots are captured model ids
    for k, seg in enumerate(narr):
        if seg.strip():
            chunks.append(seg.strip())
        if k + 1 < len(narr):  # the turns baked at hole k+1 belong right here
            d = Path("posts/runs") / slug / f"h{k + 1}"
            for p in (sorted(d.glob("*.html")) if d.is_dir() else []):
                if t := p.read_text().strip():
                    model = HASHED.sub("", p.stem).replace("__", "/")
                    chunks.append(f"--- baked h{k + 1} ({model}) ---\n{t}")
    ctx = "\n\n".join(chunks).strip()
    return (f"[{me}: this prompt is in exactly the order the reader "
            f"encounters it, prose and AI turns interleaved]\n\n{ctx}")


def main() -> None:
    ap = argparse.ArgumentParser(description="Bake AI turns for !!!org/model!!! markers.")
    ap.add_argument("post", help="post .html file")
    ap.add_argument("--hole", type=int, help="bake only this marker (1-based), using upstream turns as-is")
    ap.add_argument("--override", action="store_true", help="rebake even when nothing changed")
    ap.add_argument("--dry-run", action="store_true", help="print each prompt instead of baking")
    args = ap.parse_args()

    post, slug = Path(args.post).read_text(), Path(args.post).stem
    holes = []
    for m in MARKER.finditer(post):
        model = m.group(1).strip()
        if MODEL.fullmatch(model):
            holes.append((len(holes) + 1, model, m.start()))
    if not holes:
        sys.exit(f"{slug}: no !!!org/model!!! markers")
    if args.hole is not None and not 1 <= args.hole <= len(holes):
        sys.exit(f"h{args.hole} missing (found {len(holes)})")
    selected = holes[args.hole - 1 : args.hole] if args.hole else holes

    k = None if args.dry_run else key()
    for n, model, start in selected:
        prompt = f"you are {model}\n\n{build_ctx(post, start, slug)}"
        digest = hashlib.md5(prompt.encode()).hexdigest()
        if args.dry_run:
            print(f"--- dry run h{n}/{model} ({digest[:12]}) ---\n{prompt}")
            continue
        outdir = Path("posts/runs") / slug / f"h{n}"
        old = list(outdir.glob("*.html")) if outdir.is_dir() else []
        if old and old[0].stem.startswith(digest) and not args.override:
            print(f"h{n}/{model}: fresh ({digest[:12]}), skipping")
            continue
        print(f"h{n}/{model}: baking ({len(prompt)} chars) ...", flush=True)
        text = call(k, model, prompt).strip() + "\n"
        outdir.mkdir(parents=True, exist_ok=True)
        for p in old:
            p.unlink()
        out = outdir / f"{digest}__{model.replace('/', '__')}.html"
        out.write_text(text)
        print(f"h{n}/{model}: wrote {out}\n{text}")


if __name__ == "__main__":
    main()

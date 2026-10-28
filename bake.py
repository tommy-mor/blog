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


def read_code(inner: str) -> str | None:
    """Expand a !!!read:path[:def fn]...!!! marker to code, mirroring the
    Rust renderer (src/runs.rs): whole file, or named defs with bodies.
    Returns None for anything that isn't a read of an existing file."""
    inner = inner.removeprefix("read:")
    path, *sels = (s.strip() for s in inner.split(":"))
    p = Path(path)
    if ".." in p.parts or not p.is_file():
        return None
    text = p.read_text()
    if not sels:
        return text.strip()
    lines = text.splitlines()
    out = []
    for sel in sels:
        name = re.sub(r"[^A-Za-z0-9_]+$", "", sel.split()[-1])
        for idx, line in enumerate(lines):
            t = line.lstrip()
            head = t.split(maxsplit=1)[0] if t else ""
            if head in ("def", "fn") and t[len(head):].lstrip().startswith(name + "("):
                base = len(line) - len(t)
                out.append(line)
                for follow in lines[idx + 1:]:
                    ft = follow.strip()
                    if not ft or len(follow) - len(follow.lstrip()) > base:
                        out.append(follow)
                    else:
                        if ft and all(c in "})]" for c in ft):
                            out.append(follow)
                        break
                break
        else:
            out.append(f"[def not found: {name}]")
    return "\n".join(out)


def build_ctx(post: str, start: int, slug: str) -> str:
    """The context buildup: everything above the marker, in the reader's
    exact order -- prose, read-marker code, and baked turns interleaved at
    marker positions. Holes count model markers only; a read marker never
    shifts the numbering."""
    me = Path(__file__).name
    chunks = []
    hole = 0
    for i, part in enumerate(MARKER.split(post[:start])):
        if i % 2 == 0:  # prose between markers
            if part.strip():
                chunks.append(part.strip())
        elif MODEL.fullmatch(part.strip()):  # a hole: its baked turns land here
            hole += 1
            d = Path("posts/runs") / slug / f"h{hole}"
            for p in (sorted(d.glob("*.html")) if d.is_dir() else []):
                if t := p.read_text().strip():
                    model = HASHED.sub("", p.stem).replace("__", "/")
                    chunks.append(f"--- baked h{hole} ({model}) ---\n{t}")
        elif code := read_code(part.strip()):  # a read marker: the code itself
            chunks.append(f"--- {part.strip()} ---\n{code}")
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

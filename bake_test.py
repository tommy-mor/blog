#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = ["hypothesis", "pytest"]
# ///
"""Tests for bake.py. Run from the repo root:

    uv run bake_test.py          (or: uv run pytest bake_test.py)

Two invariants, one per test:
1. The interleave: every turn lands after the prose above its marker,
   and nothing below the hole leaks into the prompt.
2. The chain: whatever the edit/bake order, a full bake always restores
   a state where every file on disk is the output of the prompt that the
   current post and its upstream turns produce -- and a second bake is a
   no-op. The OpenRouter call is mocked to a hash of the prompt, so
   outputs change exactly when prompts do.
"""
import contextlib, hashlib, html as htmlmod, io, os, re, shutil, subprocess, sys, time, urllib.request
from pathlib import Path

import bake
from hypothesis import given, settings, strategies as st


def test_interleave_order():
    """Turns interleave after their prose; nothing below the hole leaks."""
    root = Path("posts/runs") / "__bake_selftest__"
    try:
        (root / "h1").mkdir(parents=True, exist_ok=True)
        (root / "h1" / "m__a.html").write_text("RUN-ONE")
        (root / "h2").mkdir(parents=True, exist_ok=True)
        (root / "h2" / "m__b.html").write_text("RUN-TWO")
        bang = "!" * 3
        post = bang.join(("PROSE-ONE", "x/a", "PROSE-TWO", "x/b", "PROSE-THREE"))
        ctx = bake.build_ctx(post, list(bake.MARKER.finditer(post))[1].start(), "__bake_selftest__")
        order = [ctx.find(t) for t in ("PROSE-ONE", "RUN-ONE", "PROSE-TWO")]
        assert -1 not in order and order == sorted(order), "not interleaved in reader order"
        assert "RUN-TWO" not in ctx and "PROSE-THREE" not in ctx, "leaks text below the hole"
        assert "x/a" not in ctx and "x/b" not in ctx, "captured model ids leak as chunks"
    finally:
        shutil.rmtree(root, ignore_errors=True)


def _digests_now(post_text: str, slug: str) -> dict:
    out = {}
    for m in bake.MARKER.finditer(post_text):
        model = m.group(1).strip()
        if bake.MODEL.fullmatch(model):
            prompt = f"you are {model}\n\n{bake.build_ctx(post_text, m.start(), slug)}"
            out[len(out) + 1] = hashlib.md5(prompt.encode()).hexdigest()
    return out


def _consistent(post_text: str, slug: str) -> bool:
    for n, want in _digests_now(post_text, slug).items():
        files = list((Path("posts/runs") / slug / f"h{n}").glob("*.html"))
        if len(files) != 1 or not files[0].stem.startswith(want):
            return False
    return True


@settings(max_examples=8, deadline=None, derandomize=True, database=None)
@given(st.lists(st.tuples(st.sampled_from(["e", "b", "h"]), st.integers(0, 99)),
                min_size=1, max_size=12))
def test_chain(ops):
    """Whatever the edit/bake order, a full bake heals the chain."""
    real_call, real_key, real_argv = bake.call, bake.key, sys.argv[:]
    bake.call = lambda k, model, prompt: hashlib.sha256(prompt.encode()).hexdigest()  # mock oneliner
    bake.key = lambda: "quickcheck"
    slug, post_path = "__quickcheck__", Path("__quickcheck__.html")
    bang, segs, step = "!!!", ["alpha", "bravo", "charlie", "delta", "echo"], [0]

    def text() -> str:
        parts = [segs[0]]
        for i, model in enumerate(("x/a", "x/b", "x/c", "x/d")):
            parts += [model, segs[i + 1]]
        return bang.join(parts)

    def run_main(*argv: str) -> str:
        sys.argv = ["bake.py", str(post_path), *argv]
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            bake.main()
        return buf.getvalue()

    try:
        for kind, i in ops:
            if kind == "e":  # edit prose segment i; holes below it go stale
                step[0] += 1
                segs[i % 5] += f" v{step[0]}"
                post_path.write_text(text())
                continue
            post_path.write_text(text())
            run_main(*([] if kind == "b" else ["--hole", str(i % 4 + 1)]))
            if kind == "b":  # a full bake must heal any staleness
                assert _consistent(post_path.read_text(), slug), f"chain broken after {ops}"
        run_main()
        assert _consistent(post_path.read_text(), slug), f"chain broken at end of {ops}"
        assert "baking" not in run_main(), f"second bake not a no-op after {ops}"
    finally:
        post_path.unlink(missing_ok=True)
        shutil.rmtree(Path("posts/runs") / slug, ignore_errors=True)
        bake.call, bake.key, sys.argv = real_call, real_key, real_argv


def test_read_expansion_and_hole_numbering():
    """Read markers expand to code, and never shift hole numbering:
    turns for h1 must land after the prose above h1 even when read
    markers sit between them."""
    src = Path("__read_fixture.py")
    root = Path("posts/runs") / "__read_align__"
    try:
        src.write_text("def alpha():\n    return 1\n\ndef beta():\n    return 2\n")
        (root / "h1").mkdir(parents=True, exist_ok=True)
        (root / "h1" / "m__a.html").write_text("RUN-ONE")
        bang = "!" * 3
        post = bang.join(("P0", "read:__read_fixture.py:def beta", "P1", "x/a", "P2", "x/b", "P3"))
        start = list(bake.MARKER.finditer(post))[2].start()  # bake hole 2 (x/b)
        ctx = bake.build_ctx(post, start, "__read_align__")
        tokens = ["P0", "def beta():", "P1", "RUN-ONE", "P2"]
        pos = [ctx.find(t) for t in tokens]
        assert -1 not in pos and pos == sorted(pos), f"wrong order: {list(zip(tokens, pos))}"
        assert "def alpha():" not in ctx, "selector leaked the unselected def"
        assert "P3" not in ctx and "RUN-TWO" not in ctx, "leaks below the hole"
    finally:
        src.unlink(missing_ok=True)
        shutil.rmtree(root, ignore_errors=True)


def test_dom_prompt_parity():
    """Integration: boot the real server and prove the published page and
    the model prompt are the same document -- every chunk the reader sees
    (prose, read-marker code, baked turns) appears in the prompt, in the
    same order. This is the test that would have caught read markers
    rendering in the DOM but missing from the prompt."""
    binary = Path("target/debug/blog")
    if not binary.exists():
        subprocess.run(["cargo", "build", "--quiet"], check=True)
    post_path = Path("posts/20990101_parity.html")
    src = Path("__parity_src.py")
    runs = Path("posts/runs/20990101_parity")
    db = "/tmp/parity_blog.redb"
    proc = None
    try:
        src.write_text("def parity_fn():\n    return 'PARITY-CODE-TOKEN'\n")
        bang = "!" * 3
        body = bang.join(("PARITY-PROSE-1", "read:__parity_src.py",
                          "PARITY-PROSE-2", "x/a", "PARITY-PROSE-3", "x/b", "PARITY-PROSE-4"))
        post_path.write_text(f"parity test\n{body}\n")
        full = post_path.read_text()
        marks = list(bake.MARKER.finditer(full))
        # bake hole 1 with the mock, exactly as bake.py would write it
        ctx1 = bake.build_ctx(full, marks[1].start(), "20990101_parity")
        digest = hashlib.md5(f"you are x/a\n\n{ctx1}".encode()).hexdigest()
        (runs / "h1").mkdir(parents=True)
        (runs / "h1" / f"{digest}__x__a.html").write_text("RUN-BODY-1\n")

        env = {**os.environ, "PORT": "18099", "BLOG_DB": db}
        proc = subprocess.Popen([str(binary)], env=env,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        page = None
        for _ in range(100):
            try:
                page = urllib.request.urlopen("http://localhost:18099/post/parity-test", timeout=1).read().decode()
                break
            except Exception:
                time.sleep(0.1)
        assert page, "test server never came up"

        visible = re.sub(r"<script.*?</script>", " ", page, flags=re.S)
        visible = htmlmod.unescape(re.sub(r"<[^>]+>", " ", visible))
        prompt = bake.build_ctx(full, marks[2].start(), "20990101_parity")  # prompt for hole 2

        tokens = ["PARITY-PROSE-1", "PARITY-CODE-TOKEN", "PARITY-PROSE-2",
                  "RUN-BODY-1", "PARITY-PROSE-3"]
        for name, text in (("DOM", visible), ("prompt", prompt)):
            pos = [text.find(t) for t in tokens]
            assert -1 not in pos, f"{name} is missing chunks: {[t for t, p in zip(tokens, pos) if p == -1]}"
            assert pos == sorted(pos), f"{name} order wrong: {list(zip(tokens, pos))}"
    finally:
        if proc:
            proc.kill()
        post_path.unlink(missing_ok=True)
        src.unlink(missing_ok=True)
        shutil.rmtree(runs, ignore_errors=True)
        Path(db).unlink(missing_ok=True)


if __name__ == "__main__":
    import pytest
    sys.exit(pytest.main([__file__, "-q"]))

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
import contextlib, hashlib, io, shutil, sys
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


if __name__ == "__main__":
    import pytest
    sys.exit(pytest.main([__file__, "-q"]))

//! Pre-baked model runs: each AI voice lives in its own file, rendered raw.
//! Holes are positional: the 1st marker is `h1`, etc.
//! Runs live at `posts/runs/<filestem>/h<i>/<md5>__<model>.html` and render
//! as one `<details>` per model, unescaped. The `<md5>__` prefix is the hash
//! of the baked input (cache key); pre-hash `model.html` files still load.

use std::fs;
use std::path::PathBuf;

use crate::md5::md5_hex;

/// Java `String.trim` semantics (what Clojure `str/trim` uses):
/// strip only chars <= U+0020. NOT the same as Rust's `trim()`
/// (which also strips NBSP etc.) -- hash parity with bake.clj
/// depends on getting this exactly right.
fn jtrim(s: &str) -> &str {
    s.trim_matches(|c: char| (c as u32) <= 0x20)
}

fn jtrim_start(s: &str) -> &str {
    s.trim_start_matches(|c: char| (c as u32) <= 0x20)
}

/// ASCII-whitespace split, mirroring Java `\s+` (Clojure never sees
/// Unicode spaces as separators -- verified against bb).
fn split_ascii_ws(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        let ws = matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r');
        if ws {
            if let Some(st) = start.take() {
                out.push(&s[st..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// A marker's inner text must fully match `org/model`
/// (`[\w.\-]+/[\w.\-]+`, ASCII-only classes, exactly one slash).
/// This is what bake.clj's `marker-holes` accepts; anything else is
/// prose (or a `read:` directive), never a hole.
fn valid_model(inner: &str) -> bool {
    let t = jtrim(inner);
    let mut parts = t.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), None) => {
            !a.is_empty()
                && !b.is_empty()
                && a.bytes().all(model_char)
                && b.bytes().all(model_char)
        }
        _ => false,
    }
}

fn model_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-')
}

/// Raw `!!!(.+?)!!!` runs in order, Java `Matcher.find` semantics:
/// leftmost opener, reluctant (first) same-line closer, advance past
/// the whole match. Returns (open, inner_start, inner_end, close_end)
/// byte spans. Validity is NOT checked here -- callers decide.
fn scan_pairs(content: &str) -> Vec<(usize, usize, usize, usize)> {
    let b = content.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 7 <= b.len() {
        // Find the next opener at/after pos.
        let mut open = None;
        let mut i = pos;
        while i + 3 <= b.len() {
            if &b[i..i + 3] == b"!!!" {
                open = Some(i);
                break;
            }
            i += 1;
        }
        let o = match open {
            Some(o) => o,
            None => break,
        };
        // Reluctant closer: first !!! after the opener, same line.
        // (`.` never crosses `\n`, exactly like the regex engine.)
        let mut k = o + 3;
        let mut found = None;
        while k + 3 <= b.len() && b[k] != b'\n' {
            if &b[k..k + 3] == b"!!!" {
                found = Some(k);
                break;
            }
            k += 1;
        }
        match found {
            Some(c) if c > o + 3 => {
                out.push((o, o + 3, c, c + 3));
                pos = c + 3;
            }
            _ => {
                // No usable closer here (empty or unterminated):
                // advance one char and keep scanning.
                pos = o + 1;
            }
        }
    }
    out
}

/// The hole marker: a valid `!!!org/model!!!` block.
/// Only valid runs are holes; prose `!!!` (and `read:` directives)
/// are left alone as text.
pub fn marker_spans(content: &str) -> Vec<(usize, usize)> {
    scan_pairs(content)
        .into_iter()
        .filter(|&(_, a, b, _)| valid_model(&content[a..b]))
        .map(|(o, _, _, c)| (o, c))
        .collect()
}

/// Count the holes in a post body, in order.

/// highlight.js, same CDN and version the evaleval post uses. Injected
/// once at the end of any page that spliced a code block.
pub const HLJS_SCRIPTS: &str = concat!(
    r#"<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>"#,
    r#"<script>hljs.highlightAll()</script>"#,
    r#"<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/languages/python.min.js"></script>"#,
    r#"<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/languages/rust.min.js"></script>"#,
);

/// Parse a read marker's inner text: `read:path:def a:def b` (the `read:`
/// prefix is optional). The path must name an existing file inside the
/// repo -- `..` is refused, so the DOM can never read outside it.
/// Selectors (`:def name`, `:fn name`) pick individual functions.
fn parse_read(inner: &str) -> Option<(PathBuf, Vec<String>)> {
    let inner = inner.strip_prefix("read:").unwrap_or(inner);
    let mut parts = inner.split(':');
    let path = PathBuf::from(parts.next()?.trim());
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) || !path.is_file() {
        return None;
    }
    Some((
        path,
        parts.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
    ))
}

/// hljs language class from a file extension.
fn lang_of(path: &PathBuf) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => "python",
        Some("rs") => "rust",
        Some("js") => "javascript",
        Some("html") => "xml",
        Some("css") => "css",
        Some("json") => "json",
        Some("sh") => "bash",
        Some("md") => "markdown",
        _ => "plaintext",
    }
}

/// Line-based function extraction: the `def name(` / `fn name(` line plus
/// every line indented deeper, plus a closing line of pure `})]` if one
/// stops the body. Works for python and rust alike.
fn extract_defs(text: &str, selectors: &[String]) -> String {
    if selectors.is_empty() {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    for sel in selectors {
        let name = sel
            .split_whitespace()
            .last()
            .unwrap_or(sel)
            .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
        let mut i = 0;
        let mut found = false;
        while i < lines.len() {
            let t = lines[i].trim_start();
            let head = t.split_whitespace().next().unwrap_or("");
            if (head == "def" || head == "fn") && t[head.len()..].trim_start().starts_with(&format!("{name}(")) {
                found = true;
                let base = lines[i].len() - t.len();
                out.push_str(lines[i]);
                out.push('\n');
                for follow in &lines[i + 1..] {
                    let ft = follow.trim();
                    let indent = follow.len() - follow.trim_start().len();
                    if ft.is_empty() || indent > base {
                        out.push_str(follow);
                        out.push('\n');
                    } else {
                        if ft.chars().all(|c| matches!(c, '}' | ')' | ']')) {
                            out.push_str(follow);
                            out.push('\n');
                        }
                        break;
                    }
                }
                break;
            }
            i += 1;
        }
        if !found {
            out.push_str(&format!("[def not found: {name}]\n"));
        }
    }
    out
}

fn find_marker_end(after_open: &[u8]) -> Option<usize> {
    after_open
        .windows(3)
        .position(|w| w == b"!!!")
}

/// Minimal HTML escaping for code content.
fn escape_code(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Splice `!!!read:...!!!` markers into `<pre><code>` blocks, escaped and
/// language-tagged for hljs. Returns (html, spliced_any) -- the caller
/// appends HLJS_SCRIPTS when the flag is set. Run this AFTER the run
/// splice: code on disk may contain `!!!org/model!!!` text (bake.py does),
/// and it must reach the page as code, never as a hole.
pub fn splice_reads(content: &str) -> (String, bool) {
    let b = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    let mut i = 0;
    let mut spliced = false;
    while i + 7 <= b.len() {
        if &b[i..i + 3] == b"!!!" {
            if let Some(rel) = find_marker_end(&b[i + 3..]) {
                if rel > 0 {
                    let j = i + 3 + rel + 3;
                    if let Some((path, selectors)) = parse_read(&content[i + 3..i + 3 + rel]) {
                        let code = fs::read_to_string(&path)
                            .map(|t| extract_defs(&t, &selectors))
                            .unwrap_or_else(|_| format!("[unreadable: {}]", path.display()));
                        out.push_str(&content[last..i]);
                        out.push_str(&format!(
                            "<details class=\"readcode\"><summary>{}</summary>\
                             <pre><code class=\"language-{}\">{}</code></pre></details>",
                            escape_code(content[i + 3..i + 3 + rel].trim()),
                            lang_of(&path),
                            escape_code(code.trim_end())
                        ));
                        last = j;
                        spliced = true;
                        i = j;
                        continue;
                    }
                    i += 3; // not a read marker: skip the opener, keep scanning
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push_str(&content[last..]);
    (out, spliced)
}

/// Count the holes in a post body, in order.

#[derive(Clone, Debug)]
pub struct Run {
    pub model: String,
    pub html: String,
}

fn runs_dir(slug: &str, hole: &str) -> PathBuf {
    PathBuf::from("posts").join("runs").join(slug).join(hole)
}

/// On disk `/` becomes `__` (safe in paths); in the DOM show the full key.
/// (Encoding lives in bake.py; Rust only decodes.)
pub fn decode(stem: &str) -> String {
    stem.replace("__", "/")
}

/// Splice per-hole HTML into a post body, replacing marker spans.
/// `renders[i]` is the HTML for hole `h{i+1}`, or empty when that hole
/// has no baked runs (the marker is scrubbed either way -- raw `!!!`
/// text must never reach the DOM).
pub fn splice(content: &str, renders: &[String]) -> String {
    let spans = marker_spans(content);
    if spans.is_empty() {
        return content.to_string();
    }
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    for (idx, &(s, e)) in spans.iter().enumerate() {
        out.push_str(&content[last..s]);
        if let Some(html) = renders.get(idx) {
            out.push_str(html);
        }
        last = e;
    }
    out.push_str(&content[last..]);
    out
}

/// Strip a leading `<md5>__` content-hash prefix from a baked run stem.
/// Pre-hash `model` stems pass through unchanged.
pub fn strip_hash(stem: &str) -> &str {
    let b = stem.as_bytes();
    if b.len() >= 34 && b[32] == b'_' && b[33] == b'_'
        && stem[..32].chars().all(|c| c.is_ascii_hexdigit())
    {
        &stem[34..]
    } else {
        stem
    }
}

/// (model key, file path) for every baked `.html` file in a hole dir,
/// sorted by filename so ties are deterministic.
fn run_files(slug: &str, hole: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(runs_dir(slug, hole)) else {
        return out;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".html"))
        .map(|n| n.strip_suffix(".html").unwrap_or(&n).to_string())
        .collect();
    names.sort();
    for n in names {
        let p = runs_dir(slug, hole).join(format!("{n}.html"));
        if p.is_file() {
            out.push((decode(strip_hash(&n)), p));
        }
    }
    out
}

/// Split like Java `String.split(":")`: trailing empty strings are
/// dropped, middle ones kept.
fn split_colon_java(s: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(':').collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

fn wanted_of(sel: &str) -> String {
    let last = split_ascii_ws(sel).last().copied().unwrap_or("");
    last.trim_end_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .to_string()
}

/// Mirror of bake.clj's `grab-def`: pull the `def`/`fn` block whose head
/// matches `wanted`, starting the search at `idx`.
fn bake_grab_def(lines: &[&str], idx: usize, line: &str, base: usize, sel: &str) -> Option<Vec<String>> {
    let t = jtrim_start(line);
    let head: &str = split_ascii_ws(t).first().copied().unwrap_or("");
    if !(head == "def" || head == "fn") {
        return None;
    }
    let wanted = wanted_of(sel);
    let rest = &t[head.len()..];
    if !jtrim_start(rest).starts_with(&format!("{wanted}(")) {
        return None;
    }
    let mut acc = vec![line.to_string()];
    for f in lines.iter().skip(idx + 1) {
        let ft = jtrim(f);
        let ind = f.chars().count() - jtrim_start(f).chars().count();
        if ft.is_empty() || ind > base {
            acc.push((*f).to_string());
            continue;
        }
        if ft.chars().all(|c| matches!(c, '}' | ')' | ']')) {
            acc.push((*f).to_string());
            return Some(acc);
        }
        return Some(acc);
    }
    Some(acc)
}

/// Mirror of bake.clj's `read-code`: inline a repo file (optionally
/// filtered to `def`/`fn` blocks) for a `!!!read:path[:sel]...!!!`
/// marker. `read:` prefix optional; `..` and absolute paths refused.
fn bake_read_code(inner: &str) -> Option<String> {
    let bare = inner.strip_prefix("read:").unwrap_or(inner);
    let segs = split_colon_java(bare);
    let (path, sels) = (segs.first()?, &segs[1..]);
    let path = jtrim(path);
    if path.is_empty() || path.contains('\\') || path == "/" {
        return None;
    }
    if path.split('/').any(|s| s == "..") || path.starts_with('/') {
        return None;
    }
    let p = PathBuf::from(path);
    if !p.is_file() {
        return None;
    }
    let text = jtrim(&fs::read_to_string(&p).ok()?).to_string();
    if sels.is_empty() {
        return Some(text);
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    for sel in sels {
        let mut found: Option<Vec<String>> = None;
        for (idx, line) in lines.iter().enumerate() {
            let t = jtrim_start(line);
            let base = line.chars().count() - t.chars().count();
            if let Some(block) = bake_grab_def(&lines, idx, line, base, sel) {
                found = Some(block);
                break;
            }
        }
        match found {
            Some(block) => out.extend(block),
            None => out.push(format!("[def not found: {}]", wanted_of(sel))),
        }
    }
    Some(out.join("\n"))
}

/// Rebuild the exact prompt bake.clj hashes, byte for byte: the
/// `bake.clj` header, then prose and baked voices interleaved in
/// narrative order (markers before `start` only), then the caller
/// appends `[you are {model}]`.
fn build_ctx(post: &str, start: usize, slug: &str) -> String {
    const HEADER: &str = "[bake.clj: this prompt is in exactly the order the reader encounters it, prose and AI turns interleaved]\n\n";
    let prefix = &post[..start.min(post.len())];
    let mut acc: Vec<String> = Vec::new();
    let mut hole = 0usize;
    let mut last = 0usize;
    for (o, a, b, c) in scan_pairs(prefix) {
        let t = jtrim(&prefix[last..o]);
        if !t.is_empty() {
            acc.push(t.to_string());
        }
        let inner = jtrim(&prefix[a..b]);
        if valid_model(inner) {
            hole += 1;
            let h = format!("h{hole}");
            for (model, path) in run_files(slug, &h) {
                if let Ok(text) = fs::read_to_string(&path) {
                    let t = jtrim(&text);
                    if !t.is_empty() {
                        acc.push(format!("--- baked h{hole} ({model}) ---\n{t}"));
                    }
                }
            }
        } else if let Some(code) = bake_read_code(inner) {
            acc.push(format!("--- {inner} ---\n{code}"));
        }
        last = c;
    }
    let t = jtrim(&prefix[last..]);
    if !t.is_empty() {
        acc.push(t.to_string());
    }
    format!("{HEADER}{}", jtrim(&acc.join("\n\n")))
}

/// The exact prompt bake.clj hashed for this hole and model.
pub fn expected_prompt(content: &str, slug: &str, hole_idx: usize, model: &str) -> String {
    let spans = marker_spans(content);
    let start = spans.get(hole_idx).map(|(s, _)| *s).unwrap_or(content.len());
    format!("{}[you are {model}]\n\n", build_ctx(content, start, slug))
}

/// The Nth valid marker's model (trimmed inner text), or None.
fn hole_model(content: &str, hole_idx: usize) -> Option<String> {
    let mut seen = 0usize;
    for (_, a, b, _) in scan_pairs(content) {
        let inner = jtrim(&content[a..b]);
        if valid_model(inner) {
            if seen == hole_idx {
                return Some(inner.to_string());
            }
            seen += 1;
        }
    }
    None
}

/// Every baked run for a hole whose filename starts with the prompt
/// hash bake.clj would compute right now. Stale bakes (edited context,
/// changed earlier voices) are skipped: they render nothing until
/// rebaked. Mirrors bake-one's `starts-with` freshness check.
pub fn fresh_runs(slug: &str, hole_idx: usize, content: &str) -> Vec<Run> {
    let hole = format!("h{}", hole_idx + 1);
    let spans = marker_spans(content);
    if spans.get(hole_idx).is_none() {
        return Vec::new();
    }
    // The marker's own model goes on the identity line -- exact, no
    // lossy filename decoding involved.
    let Some(model) = hole_model(content, hole_idx) else {
        return Vec::new();
    };
    let prompt = expected_prompt(content, slug, hole_idx, &model);
    let digest = md5_hex(prompt.as_bytes());
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(runs_dir(slug, &hole)) else {
        return out;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".html"))
        .map(|n| n.strip_suffix(".html").unwrap_or(&n).to_string())
        .collect();
    names.sort();
    for n in names {
        if !n.starts_with(&digest) {
            continue;
        }
        let p = runs_dir(slug, &hole).join(format!("{n}.html"));
        if let Ok(text) = fs::read_to_string(&p) {
            let html = text.trim().to_string();
            if !html.is_empty() {
                out.push(Run {
                    model: model.clone(),
                    html,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holes_are_positional() {
        let body = "<p>one</p>!!!a/m!!!<p>two</p>!!!org/m!!!<p>three</p>!!!x/y!!!";
        let spans = marker_spans(body);
        assert_eq!(spans.len(), 3);
        // hole hN covers the Nth marker in narrative order
        assert!(spans[0].1 <= spans[1].0 && spans[1].1 <= spans[2].0);
    }

    #[test]
    fn marker_spans_find_model_blocks() {
        let body = "a!!!m/n!!!b!!!org/m!!!c";
        assert_eq!(marker_spans(body), vec![(1, 10), (11, 22)]);
        // Angle brackets are plain text and AI is not a model: no marker.
        assert!(marker_spans("a<!!!AI!!!>b").is_empty());
        // Empty !!!!!! is not a marker; unterminated !!! isn't either.
        assert!(marker_spans("a!!!!!!b").is_empty());
        assert!(marker_spans("a!!!oops b").is_empty());
    }

    #[test]
    fn marker_spans_ignore_prose_bangs() {
        // A prose !!! must not pair with the next real marker and
        // swallow the paragraphs between them.
        let body = "two!!!\n\n<p>mid</p>\n!!!a/b!!!\nend";
        assert_eq!(marker_spans(&body).len(), 1);
        let out = splice(&body, &[String::from("<X>")]);
        assert!(out.contains("two!!!"));
        assert!(!out.contains("!!!a/b!!!"));
    }

    #[test]
    fn holes_replace_in_order() {
        // Same splice as post_page: h1 lands in the 1st hole, h2 in the 2nd.
        let content = String::from("a!!!m/n!!!b!!!org/m!!!c");
        assert_eq!(
            splice(&content, &["<1>".to_string(), "<2>".to_string()]),
            "a<1>b<2>c"
        );
    }

    #[test]
    fn splice_scrubs_unbaked_holes() {
        // No runs -> no raw !!! text may survive into the DOM.
        let content = String::from("a!!!m/n!!!b!!!org/m!!!c");
        let out = splice(&content, &[String::new(), "<2>".to_string()]);
        assert_eq!(out, "ab<2>c");
        assert!(!out.contains("!!!"));
    }

    #[test]
    fn fresh_runs_renders_only_matching_digest() {
        // Files that don't hash-match the current prompt are invisible:
        // a stale same-model bake sorting first, an unhashed legacy file,
        // and another model's bake must all be skipped.
        let body = "Hello !!!org/m1!!! tail";
        let slug = "__test_fresh__";
        let dir = PathBuf::from("posts/runs/__test_fresh__/h1");
        fs::create_dir_all(&dir).unwrap();
        let prompt = expected_prompt(body, slug, 0, "org/m1");
        let digest = crate::md5::md5_hex(prompt.as_bytes());
        fs::write(dir.join("00000000000000000000000000000000__org__m1.html"), "<p>STALE</p>").unwrap();
        fs::write(dir.join("org__other.html"), "<p>OTHER</p>").unwrap();
        fs::write(dir.join(format!("{digest}__org__m1.html")), "<p>FRESH</p>").unwrap();
        let got = fresh_runs(slug, 0, body);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].model, "org/m1");
        assert_eq!(got[0].html, "<p>FRESH</p>");
        fs::remove_dir_all("posts/runs/__test_fresh__").unwrap();
    }

    #[test]
    fn decode_handles_slashes() {
        assert_eq!(decode("anthropic__claude-fable"), "anthropic/claude-fable");
        assert_eq!(decode("org__model-x"), "org/model-x");
    }

    #[test]
    fn stale_prompt_change_unrenders() {
        // Editing narrative above the marker changes the digest, so the
        // old bake stops rendering until rebaked.
        let slug = "__test_stale__";
        let dir = PathBuf::from("posts/runs/__test_stale__/h1");
        fs::create_dir_all(&dir).unwrap();
        let body_v1 = String::from("Hello !!!org/m1!!! tail");
        let d1 = crate::md5::md5_hex(expected_prompt(&body_v1, slug, 0, "org/m1").as_bytes());
        fs::write(dir.join(format!("{d1}__org__m1.html")), "<p>V1</p>").unwrap();
        assert_eq!(fresh_runs(slug, 0, &body_v1).len(), 1);
        let body_v2 = String::from("Hello edited !!!org/m1!!! tail");
        assert!(fresh_runs(slug, 0, &body_v2).is_empty());
        fs::remove_dir_all("posts/runs/__test_stale__").unwrap();
    }

    #[test]
    fn strip_hash_passes_plain_stems_through() {
        assert_eq!(strip_hash("org__m"), "org__m");
        assert_eq!(strip_hash("tooshort__org__m"), "tooshort__org__m");
        assert_eq!(
            strip_hash("d41d8cd98f00b204e9800998ecf8427e__org__m"),
            "org__m"
        );
    }

    #[test]
    fn prompt_matches_bake_clj_byte_for_byte() {
        // Fixture twin of posts/__test_bb__.html (slug __test_bb__).
        // Digests below are what `bb bake.clj --dry-run` reports for
        // this exact fixture -- regenerate them that way if it changes:
        //   bb bake.clj posts/__test_bb__.html --dry-run
        let body = "Hello !!!org/m1!!! middle !!!read:__test_bb_read.txt!!! tail !!!org/m2!!! end";
        let slug = "__test_bb__";
        let dir = PathBuf::from("posts/runs/__test_bb__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(PathBuf::from("__test_bb_read.txt"), "READ-BLOCK-CONTENT").unwrap();
        fs::write(
            dir.join("25c2ea105bb5c4d09b96d03ff7420593__org__m1.html"),
            "<p>FRESH-ONE</p>",
        )
        .unwrap();
        let p1 = expected_prompt(body, slug, 0, "org/m1");
        assert_eq!(
            p1,
            "[bake.clj: this prompt is in exactly the order the reader encounters it, prose and AI turns interleaved]\n\nHello[you are org/m1]\n\n"
        );
        assert_eq!(crate::md5::md5_hex(p1.as_bytes()), "25c2ea105bb5c4d09b96d03ff7420593");
        let p2 = expected_prompt(body, slug, 1, "org/m2");
        assert_eq!(
            p2,
            "[bake.clj: this prompt is in exactly the order the reader encounters it, prose and AI turns interleaved]\n\nHello\n\n--- baked h1 (org/m1) ---\n<p>FRESH-ONE</p>\n\nmiddle\n\n--- read:__test_bb_read.txt ---\nREAD-BLOCK-CONTENT\n\ntail[you are org/m2]\n\n"
        );
        assert_eq!(crate::md5::md5_hex(p2.as_bytes()), "5626e63fd000e90bfb03f83d2a67ff40");
        let got = fresh_runs(slug, 0, body);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].model, "org/m1");
        fs::remove_dir_all("posts/runs/__test_bb__").unwrap();
        fs::remove_file(PathBuf::from("__test_bb_read.txt")).unwrap();
    }

    #[test]
    fn spliced_dom_matches_bake_interleave_order() {
        // Same full render as post_page: per-hole runcells spliced at their
        // markers. The DOM reading order (narrative segments interleaved
        // with each hole's runs in loads() order) must match the chunk
        // order bake.py's dom_order_chunks/assert_word_order enforces.
        let root = PathBuf::from("posts/runs/__test_interleave__");
        let h1 = root.join("h1");
        let h2 = root.join("h2");
        fs::create_dir_all(&h1).unwrap();
        fs::create_dir_all(&h2).unwrap();
        let body = "NARR-1!!!x/y!!!NARR-2!!!u/v!!!NARR-3";
        // Bake in narrative order like bake --all does: h2's prompt is
        // hashed with h1's bake already on disk.
        for (idx, model, tok) in [(0usize, "x/y", "RUN-A"), (1usize, "u/v", "RUN-C")] {
            let d = crate::md5::md5_hex(expected_prompt(body, "__test_interleave__", idx, model).as_bytes());
            let enc = model.replace('/', "__");
            fs::write(root.join(format!("h{}", idx + 1)).join(format!("{d}__{enc}.html")), format!("<p>{tok}</p>")).unwrap();
        }
        let render = |body: &str| {
            let renders: Vec<String> = marker_spans(body)
                .iter()
                .enumerate()
                .map(|(idx, _)| {
                    let hole = format!("h{}", idx + 1);
                    let baked = fresh_runs("__test_interleave__", idx, body);
                    crate::templates::runcell(&hole, &baked).into_string()
                })
                .collect();
            splice(body, &renders)
        };
        let dom = render(body);
        let mut pos = 0;
        for tok in ["NARR-1", "RUN-A", "NARR-2", "RUN-C", "NARR-3"] {
            let at = dom.find(tok).unwrap_or_else(|| panic!("{tok} missing from DOM"));
            assert!(at >= pos, "{tok} out of order in DOM: {dom}");
            pos = at;
        }
        // A stale file landing in h1 afterwards poisons h2's context
        // (same as a real edit would): h1 still renders, h2 correctly
        // hides until the cascade is rebaked.
        fs::write(h1.join("00000000000000000000000000000000__x__y.html"), "<p>RUN-B</p>").unwrap();
        let dom = render(body);
        assert!(dom.contains("RUN-A"), "fresh h1 must survive stray files");
        assert!(!dom.contains("RUN-B"), "stale bake leaked into DOM");
        assert!(!dom.contains("RUN-C"), "h2 must hide on poisoned context");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn read_markers_render_code_blocks() {
        let f = PathBuf::from("__read_fixture_a.py");
        fs::write(&f, "def alpha():\n    return 'a < b & c'\n\ndef beta():\n    return 2\n").unwrap();
        let (out, spliced) = splice_reads("before !!!read:__read_fixture_a.py!!! after");
        assert!(spliced);
        assert!(out.contains("<details class=\"readcode\"><summary>read:__read_fixture_a.py</summary>"));
        assert!(!out.contains("readcode\" open"), "code blocks must start collapsed");
        assert!(out.contains("<pre><code class=\"language-python\">"));
        assert!(out.contains("&#x27;" ) || out.contains("'a &lt; b &amp; c'"));
        assert!(out.starts_with("before ") && out.ends_with(" after"));
        fs::remove_file(&f).unwrap();
    }

    #[test]
    fn read_selectors_pick_single_defs() {
        let f = PathBuf::from("__read_fixture_b.py");
        fs::write(&f, "def alpha():\n    return 1\n\ndef beta():\n    return 2\n").unwrap();
        let (out, _) = splice_reads("!!!__read_fixture_b.py:def beta!!!");
        assert!(out.contains("def beta():"));
        assert!(out.contains("return 2"));
        assert!(!out.contains("def alpha():"), "alpha leaked into beta's selection");
        let (out, _) = splice_reads("!!!read:__read_fixture_b.py:def gamma!!!");
        assert!(out.contains("[def not found: gamma]"));
        fs::remove_file(&f).unwrap();
    }

    #[test]
    fn read_markers_refuse_escape_and_prose() {
        // `..` paths and nonexistent files are left as plain text.
        let (out, spliced) = splice_reads("x !!!read:../etc/passwd!!! y !!!not a file!!! z");
        assert!(!spliced);
        assert!(out.contains("!!!read:../etc/passwd!!!"));
        assert!(out.contains("!!!not a file!!!"));
    }

    #[test]
    fn code_containing_model_markers_survives_as_code() {
        // bake.py's own source contains `!!!org/model!!!` text. The run
        // splice happens first on the original post; the read splice runs
        // second, so marker-looking text inside a file is never a hole.
        let f = PathBuf::from("__read_fixture_c.py");
        fs::write(&f, "# sees !!!x/y!!! markers\n").unwrap();
        let post = format!("narrative !!!a/b!!! then !!!read:{}!!!", f.display());
        let rendered = splice(&post, &["<RUNCELL>".to_string()]);
        let (out, had_reads) = splice_reads(&rendered);
        assert!(had_reads);
        assert!(out.contains("<RUNCELL>"));
        assert!(out.contains("# sees !!!x/y!!! markers"));
        fs::remove_file(&f).unwrap();
    }

    #[test]
    #[ignore = "explicit freshness audit: run with `cargo test -- --ignored`. Fails when the post changed without a rebake."]
    fn committed_runs_match_current_prompts() {
        // Every baked file on disk must hash-match the prompt the
        // renderer will rebuild. If you edited the post, rebake first:
        //   bb bake.clj posts/20261024_rlbot.html
        let post = fs::read_to_string("posts/20261024_rlbot.html").unwrap();
        let slug = "20261024_rlbot";
        let mut checked = 0;
        for (idx, _) in marker_spans(&post).iter().enumerate() {
            let Some(model) = hole_model(&post, idx) else {
                continue;
            };
            let digest = md5_hex(expected_prompt(&post, slug, idx, &model).as_bytes());
            let dir = runs_dir(slug, &format!("h{}", idx + 1));
            if let Ok(entries) = fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if !name.ends_with(".html") {
                        continue;
                    }
                    let stem = name.trim_end_matches(".html");
                    assert!(
                        stem.starts_with(&digest as &str),
                        "stale bake {} (hole h{}): post changed, rebake",
                        name,
                        idx + 1
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no baked runs found to audit");
    }

    #[test]
    fn prompt_matches_bake_clj_selectors() {
        // `!!!read:file:def name!!!` parity, digests from bb dry-run:
        //   printf 'def alpha():\n    return 1\n\ndef beta(arg):\n    return arg\n' > __test_sel.txt
        //   printf 'line one\n!!!o/m!!!\nmid !!!read:__test_sel.txt:def beta!!! tail !!!o/n!!! end\n' > posts/__test_sel__.html
        //   bb bake.clj posts/__test_sel__.html --dry-run
        let body = "line one\n!!!o/m!!!\nmid !!!read:__test_sel.txt:def beta!!! tail !!!o/n!!! end";
        let slug = "__test_sel__";
        fs::write(PathBuf::from("__test_sel.txt"), "def alpha():\n    return 1\n\ndef beta(arg):\n    return arg\n").unwrap();
        let p1 = expected_prompt(body, slug, 0, "o/m");
        assert_eq!(
            p1,
            "[bake.clj: this prompt is in exactly the order the reader encounters it, prose and AI turns interleaved]\n\nline one[you are o/m]\n\n"
        );
        assert_eq!(crate::md5::md5_hex(p1.as_bytes()), "efdb02138059f3b2ba9f7a90db0f727b");
        let p2 = expected_prompt(body, slug, 1, "o/n");
        assert_eq!(
            p2,
            "[bake.clj: this prompt is in exactly the order the reader encounters it, prose and AI turns interleaved]\n\nline one\n\nmid\n\n--- read:__test_sel.txt:def beta ---\ndef beta(arg):\n    return arg\n\ntail[you are o/n]\n\n"
        );
        assert_eq!(crate::md5::md5_hex(p2.as_bytes()), "d7d67711b8d9d183d684b0311b1c1bf6");
        fs::remove_file(PathBuf::from("__test_sel.txt")).unwrap();
    }

    #[test]
    fn fresh_runs_rejects_missing_and_empty() {
        let body = "narrative !!!org/m!!! tail";
        assert!(fresh_runs("__no_such_post__", 0, body).is_empty());
        let dir = PathBuf::from("posts/runs/__test_empty__/h1");
        fs::create_dir_all(&dir).unwrap();
        let d = crate::md5::md5_hex(expected_prompt(body, "__test_empty__", 0, "org/m").as_bytes());
        fs::write(dir.join(format!("{d}__org__m.html")), "  \n ").unwrap();
        assert!(fresh_runs("__test_empty__", 0, body).is_empty());
        fs::remove_dir_all("posts/runs/__test_empty__").unwrap();
    }
}

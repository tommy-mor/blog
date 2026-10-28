//! Pre-baked model runs: each AI voice lives in its own file, rendered raw.
//! Holes are positional: the 1st marker is `h1`, etc.
//! Runs live at `posts/runs/<filestem>/h<i>/<md5>__<model>.html` and render
//! as one `<details>` per model, unescaped. The `<md5>__` prefix is the hash
//! of the baked input (cache key); pre-hash `model.html` files still load.

use std::fs;
use std::path::PathBuf;

/// A marker's inner text must look like an openrouter id
/// (`org/model` -- no whitespace, exactly the model path). This keeps
/// prose exclamations (`a thing or two!!!`) from pairing with the next
/// real marker and swallowing everything between them.
fn valid_marker_inner(inner: &str) -> bool {
    !inner.is_empty()
        && inner.contains('/')
        && inner
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b'/'))
}

/// The hole marker: a `!!!org/model!!!` block names its model
/// (e.g. `!!!openai/gpt-6-luna!!!`). Only valid runs are holes;
/// prose `!!!` that doesn't name a model is left alone as text.
pub fn marker_spans(content: &str) -> Vec<(usize, usize)> {
    let b = content.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i + 7 <= b.len() {
        if &b[i..i + 3] == b"!!!" {
            if let Some(rel) = find_marker_end(&b[i + 3..]) {
                if rel == 0 {
                    // Empty `!!!!!!`: not a marker.
                    i += 1;
                    continue;
                }
                let j = i + 3 + rel + 3;
                if valid_marker_inner(&content[i + 3..i + 3 + rel]) {
                    spans.push((i, j));
                    i = j;
                    continue;
                }
                // Looks like punctuation, not a model: skip the opener
                // and keep scanning (the closer may open a real marker).
                i += 3;
                continue;
            }
        }
        i += 1;
    }
    spans
}

fn find_marker_end(after_open: &[u8]) -> Option<usize> {
    after_open
        .windows(3)
        .position(|w| w == b"!!!")
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
    if b.len() > 34 && b[32] == b'_' && b[33] == b'_'
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
        .map(|n| n.trim_end_matches(".html").to_string())
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

/// Models with a baked file for this hole, in filename order.
/// Returns full openrouter ids (`__` decoded to `/`, hash prefix stripped).
pub fn models_for(slug: &str, hole: &str) -> Vec<String> {
    let mut out: Vec<String> = run_files(slug, hole)
        .into_iter()
        .map(|(model, _)| model)
        .collect();
    out.dedup();
    out
}

/// Load one run: raw file content, unescaped at render.
/// `model` is the full openrouter id (`org/model`); the file may carry a
/// `<md5>__` prefix which is ignored on read.
pub fn load(slug: &str, hole: &str, model: &str) -> Option<Run> {
    let path = run_files(slug, hole)
        .into_iter()
        .find(|(m, _)| m == model)
        .map(|(_, p)| p)?;
    let text = fs::read_to_string(path).ok()?;
    let html = text.trim().to_string();
    if html.is_empty() {
        return None;
    }
    Some(Run {
        model: model.to_string(),
        html,
    })
}

/// Every baked run for a hole, in model order.
pub fn loads(slug: &str, hole: &str) -> Vec<Run> {
    models_for(slug, hole)
        .iter()
        .filter_map(|m| load(slug, hole, m))
        .collect()
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
    fn loads_reads_html_files_in_order() {
        let dir = PathBuf::from("posts/runs/__test_hole__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("b.html"), "<p>B</p>").unwrap();
        fs::write(dir.join("a.html"), "<p>A</p>").unwrap();
        fs::write(dir.join("ignore.txt"), "nope").unwrap();
        let got = loads("__test_hole__", "h1");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].model, "a");
        assert_eq!(got[0].html, "<p>A</p>");
        assert_eq!(got[1].model, "b");
        fs::remove_dir_all("posts/runs/__test_hole__").unwrap();
    }

    #[test]
    fn decode_handles_slashes() {
        assert_eq!(decode("anthropic__claude-fable"), "anthropic/claude-fable");
        let dir = PathBuf::from("posts/runs/__test_enc__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("org__model-x.html"), "<p>hi</p>").unwrap();
        let got = loads("__test_enc__", "h1");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].model, "org/model-x");
        fs::remove_dir_all("posts/runs/__test_enc__").unwrap();
    }

    #[test]
    fn loads_reads_hashed_names() {
        let dir = PathBuf::from("posts/runs/__test_hash__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("d41d8cd98f00b204e9800998ecf8427e__org__model-x.html"), "<p>hi</p>").unwrap();
        fs::write(dir.join("org__model-y.html"), "<p>yo</p>").unwrap();
        let got = loads("__test_hash__", "h1");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].model, "org/model-x");
        assert_eq!(got[1].model, "org/model-y");
        assert!(load("__test_hash__", "h1", "org/model-x").is_some());
        fs::remove_dir_all("posts/runs/__test_hash__").unwrap();
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
    fn loads_all_models_in_a_hole() {
        // A comparison drawer: every baked model renders, not just the first.
        let dir = PathBuf::from("posts/runs/__test_multi__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa__org__a.html"), "<p>A</p>").unwrap();
        fs::write(dir.join("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb__org__b.html"), "<p>B</p>").unwrap();
        let got = loads("__test_multi__", "h1");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].model, "org/a");
        assert_eq!(got[1].model, "org/b");
        assert!(load("__test_multi__", "h1", "org/b").is_some());
        fs::remove_dir_all("posts/runs/__test_multi__").unwrap();
    }

    #[test]
    fn loads_orders_by_raw_filename_not_model() {
        // Cross-language contract with bake.py's hole_outputs: runs sort by
        // raw filename, hash prefix included. Here raw order
        // (aaaa..org__z < bbbb..org__a) disagrees with stripped-stem order
        // (org/a < org/z); raw filename must win or the DOM order drifts
        // from the prompt order assert_word_order checks.
        let dir = PathBuf::from("posts/runs/__test_order__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb__org__a.html"), "<p>A</p>").unwrap();
        fs::write(dir.join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa__org__z.html"), "<p>Z</p>").unwrap();
        let got = loads("__test_order__", "h1");
        assert_eq!(
            got.iter().map(|r| r.model.as_str()).collect::<Vec<_>>(),
            vec!["org/z", "org/a"]
        );
        fs::remove_dir_all("posts/runs/__test_order__").unwrap();
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
        fs::write(h1.join("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb__org__a.html"), "<p>RUN-B</p>").unwrap();
        fs::write(h1.join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa__org__z.html"), "<p>RUN-A</p>").unwrap();
        fs::write(h2.join("org__c.html"), "<p>RUN-C</p>").unwrap();
        let body = "NARR-1!!!x/y!!!NARR-2!!!u/v!!!NARR-3";
        let renders: Vec<String> = marker_spans(body)
            .iter()
            .enumerate()
            .map(|(idx, _)| {
                let hole = format!("h{}", idx + 1);
                let baked = loads("__test_interleave__", &hole);
                crate::templates::runcell(&hole, &baked).into_string()
            })
            .collect();
        let dom = splice(body, &renders);
        fs::remove_dir_all(&root).unwrap();
        let mut pos = 0;
        for tok in ["NARR-1", "RUN-A", "RUN-B", "NARR-2", "RUN-C", "NARR-3"] {
            let at = dom.find(tok).unwrap_or_else(|| panic!("{tok} missing from DOM"));
            assert!(at >= pos, "{tok} out of order in DOM: {dom}");
            pos = at;
        }
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
    fn load_rejects_missing_and_empty() {
        assert!(load("__no_such_post__", "h1", "x").is_none());
        let dir = PathBuf::from("posts/runs/__test_empty__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("e.html"), "  \n ").unwrap();
        assert!(load("__test_empty__", "h1", "e").is_none());
        fs::remove_dir_all("posts/runs/__test_empty__").unwrap();
    }
}

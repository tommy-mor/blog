//! Pre-baked model runs: each AI voice lives in its own file, rendered raw.
//! Holes are positional: the 1st marker is `h1`, etc.
//! Runs live at `posts/runs/<filestem>/h<i>/<md5>__<model>.html` and render
//! as one `<details>` per model, unescaped. The `<md5>__` prefix is the hash
//! of the baked input (cache key); pre-hash `model.html` files still load.

use std::fs;
use std::path::PathBuf;

/// The hole marker: legacy `<!!!AI!!!>`, or new-style `!!!model!!!`
/// where the block names its model (e.g. `!!!openai/gpt-6-luna!!!`).
/// New-style is any `!!!...!!!` run NOT immediately wrapped in `<`/`>`.
pub fn marker_spans(content: &str) -> Vec<(usize, usize)> {
    let b = content.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i + 7 <= b.len() {
        if &b[i..i + 3] == b"!!!" {
            if let Some(rel) = find_marker_end(&b[i + 3..]) {
                let inner_end = i + 3 + rel;
                let j = inner_end + 3;
                if inner_end == i + 3 {
                    // Empty `!!!!!!`: not a marker.
                    i += 1;
                    continue;
                }
                let wrapped = i > 0 && b[i - 1] == b'<' && j < b.len() && b[j] == b'>';
                if wrapped {
                    spans.push((i - 1, j + 1));
                    i = j + 1;
                } else {
                    spans.push((i, j));
                    i = j;
                }
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

#[derive(Clone, Debug)]
pub struct Run {
    pub model: String,
    pub html: String,
}

fn runs_dir(slug: &str, hole: &str) -> PathBuf {
    PathBuf::from("posts").join("runs").join(slug).join(hole)
}

/// On disk `/` becomes `__` (safe in paths); in the DOM show the full key.
pub fn encode(model: &str) -> String {
    model.replace('/', "__")
}
pub fn decode(stem: &str) -> String {
    stem.replace("__", "/")
}

/// Count the holes in a post body, in order.
pub fn hole_count(content: &str) -> usize {
    marker_spans(content).len()
}

/// Hole ids for a body: `h1..=hN` in narrative order.
pub fn hole_ids(content: &str) -> Vec<String> {
    (1..=hole_count(content)).map(|i| format!("h{i}")).collect()
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
        let body = "<p>one</p><!!!AI!!!><p>two</p>!!!org/m!!!<p>three</p>!!!x/y!!!";
        assert_eq!(hole_count(body), 3);
        assert_eq!(hole_ids(body), vec!["h1", "h2", "h3"]);
    }

    #[test]
    fn marker_spans_find_both_syntaxes() {
        let body = "a<!!!AI!!!>b!!!org/m!!!c";
        assert_eq!(marker_spans(body), vec![(1, 11), (12, 23)]);
        // Empty !!!!!! is not a marker; unterminated !!! isn't either.
        assert!(marker_spans("a!!!!!!b").is_empty());
        assert!(marker_spans("a!!!oops b").is_empty());
    }

    #[test]
    fn holes_replace_in_order() {
        // Same splice loop as post_page: h1 lands in the 1st hole, h2 in the 2nd.
        let content = String::from("a<!!!AI!!!>b!!!org/m!!!c");
        let spans = marker_spans(&content);
        let mut out = String::new();
        let mut last = 0;
        for (val, &(s, e)) in ["<1>", "<2>"].iter().zip(spans.iter()) {
            out.push_str(&content[last..s]);
            out.push_str(val);
            last = e;
        }
        out.push_str(&content[last..]);
        assert_eq!(out, "a<1>b<2>c");
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
    fn encode_roundtrips_slash() {
        assert_eq!(encode("anthropic/claude-fable"), "anthropic__claude-fable");
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
    fn load_rejects_missing_and_empty() {
        assert!(load("__no_such_post__", "h1", "x").is_none());
        let dir = PathBuf::from("posts/runs/__test_empty__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("e.html"), "  \n ").unwrap();
        assert!(load("__test_empty__", "h1", "e").is_none());
        fs::remove_dir_all("posts/runs/__test_empty__").unwrap();
    }
}

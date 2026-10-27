//! Pre-baked model runs: each AI voice lives in its own file, rendered raw.
//! Holes are positional: the 1st `<!!!AI!!!>` is `h1`, etc.
//! Runs live at `posts/runs/<filestem>/h<i>/<model>.html` and render as one
//! `<details>` per model, unescaped.

use std::fs;
use std::path::PathBuf;

/// The hole marker: where the narrative wants an AI to speak.
pub const HOLE: &str = "<!!!AI!!!>";

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
    content.matches(HOLE).count()
}

/// Hole ids for a body: `h1..=hN` in narrative order.
pub fn hole_ids(content: &str) -> Vec<String> {
    (1..=hole_count(content)).map(|i| format!("h{i}")).collect()
}

/// Models with a baked file for this hole, in filename order.
/// Returns full openrouter ids (`__` decoded to `/`).
pub fn models_for(slug: &str, hole: &str) -> Vec<String> {
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
    names.dedup();
    for n in names {
        let p = runs_dir(slug, hole).join(format!("{n}.html"));
        if p.is_file() {
            out.push(decode(&n));
        }
    }
    out
}

/// Load one run: raw file content, unescaped at render.
/// `model` is the full openrouter id (`org/model`); the file uses `__`.
pub fn load(slug: &str, hole: &str, model: &str) -> Option<Run> {
    let text = fs::read_to_string(runs_dir(slug, hole).join(format!("{}.html", encode(model)))).ok()?;
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
        let body = "<p>one</p><!!!AI!!!><p>two</p><!!!AI!!!><p>three</p>";
        assert_eq!(hole_count(body), 2);
        assert_eq!(hole_ids(body), vec!["h1", "h2"]);
    }

    #[test]
    fn holes_replace_in_order() {
        // Same replacen loop as post_page: h1 lands in the 1st hole, h2 in the 2nd.
        let mut content = String::from("a<!!!AI!!!>b<!!!AI!!!>c");
        for val in ["<1>", "<2>"] {
            content = content.replacen(HOLE, val, 1);
        }
        assert_eq!(content, "a<1>b<2>c");
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
    fn load_rejects_missing_and_empty() {
        assert!(load("__no_such_post__", "h1", "x").is_none());
        let dir = PathBuf::from("posts/runs/__test_empty__/h1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("e.html"), "  \n ").unwrap();
        assert!(load("__test_empty__", "h1", "e").is_none());
        fs::remove_dir_all("posts/runs/__test_empty__").unwrap();
    }
}

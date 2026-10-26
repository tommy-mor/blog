//! Pre-baked model runs: each AI voice lives in its own file, paragraphs
//! split on blank lines. No generation, no keys, no billing at pageview —
//! the reader picks a voice, the server reads a file.
//!
//! The source post HTML holds `<!!!AI!!!>` holes where the narrative wants
//! an AI interjection. Holes are positional: the 1st marker is hole `h1`,
//! the 2nd is `h2`, and so on. Runs live at
//! `posts/runs/<slug>/h<i>/<model>.md` (`fable.md`, `kimi-k3.md`, ...).
//! The bake step feeds each model everything above its hole; the render
//! step replaces each hole with the selected voice, collapsible per
//! paragraph. Curated and scrubbed by Master before anything lands here.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// The hole marker: where the narrative wants an AI to speak.
pub const HOLE: &str = "<!!!AI!!!>";

/// One pre-baked run: paragraphs in file order.
#[derive(Clone, Debug)]
pub struct Run {
    pub model: String,
    pub paragraphs: Vec<String>,
}

fn runs_dir(slug: &str, hole: &str) -> PathBuf {
    PathBuf::from("posts").join("runs").join(slug).join(hole)
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
pub fn models_for(slug: &str, hole: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(runs_dir(slug, hole)) else {
        return out;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .map(|n| n.trim_end_matches(".md").to_string())
        .collect();
    names.sort();
    names.dedup();
    for n in names {
        let p = runs_dir(slug, hole).join(format!("{n}.md"));
        if p.is_file() {
            out.push(n);
        }
    }
    out
}

/// Load one run: split on blank lines, drop empties, keep order.
pub fn load(slug: &str, hole: &str, model: &str) -> Option<Run> {
    let path = runs_dir(slug, hole).join(format!("{model}.md"));
    let text = fs::read_to_string(path).ok()?;
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if paragraphs.is_empty() {
        return None;
    }
    Some(Run {
        model: model.to_string(),
        paragraphs,
    })
}

/// Every hole in the post that has at least one baked run:
/// hole id → models. Only holes with files render a picker; bare holes
/// (no runs yet) render nothing — the narrative flows past them.
pub fn holes_for(slug: &str, content: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for hole in hole_ids(content) {
        let models = models_for(slug, &hole);
        if !models.is_empty() {
            out.insert(hole, models);
        }
    }
    out
}

/// Strip tags for bake context: the AI reads the narrative, not the markup.
pub fn text_above(content: &str, hole_index: usize) -> String {
    let mut parts = content.split(HOLE);
    let mut above = String::new();
    for _ in 0..hole_index {
        if let Some(chunk) = parts.next() {
            above.push_str(chunk);
            above.push('\n');
        }
    }
    strip_tags(&above).trim().to_string()
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
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
    fn context_is_everything_above() {
        let body = "<p>setup</p><!!!AI!!!><p>middle</p><!!!AI!!!><p>end</p>";
        assert_eq!(text_above(body, 1), "setup");
        assert!(text_above(body, 2).contains("setup"));
        assert!(text_above(body, 2).contains("middle"));
        assert!(!text_above(body, 2).contains("end"));
    }

    #[test]
    fn splits_paragraphs_on_blank_lines() {
        let text = "one\n\ntwo\nthree\n\n\nfour\n";
        let paras: Vec<String> = text
            .split("\n\n")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        assert_eq!(paras, vec!["one", "two\nthree", "four"]);
    }
}

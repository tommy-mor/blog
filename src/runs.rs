//! Pre-baked model runs: each AI voice lives in its own file, rendered raw.
//! Holes are positional: the 1st `<!!!AI!!!>` is `h1`, etc.
//! Runs live at `posts/runs/<filestem>/h<i>/<model>.md` and render as one
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

/// Load one run: raw file content, unescaped at render.
pub fn load(slug: &str, hole: &str, model: &str) -> Option<Run> {
    let text = fs::read_to_string(runs_dir(slug, hole).join(format!("{model}.md"))).ok()?;
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
}

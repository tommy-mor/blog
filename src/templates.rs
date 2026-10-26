use crate::posts::Post;
use maud::{html, Markup, PreEscaped, DOCTYPE};

fn base(title: &str, slug: Option<&str>, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                link rel="stylesheet" href="/static/style.css";
            }
            @if let Some(s) = slug {
                body data-slug=(s) {
                    (page_inner(content))
                }
            } @else {
                body {
                    (page_inner(content))
                }
            }
        }
    }
}

fn page_inner(content: Markup) -> Markup {
    html! {
        h1.fronttitle {
            a.hilite href="/" title="by Tommy Morriss" { "Blog" }
        }
        (content)
    }
}

pub fn index(posts: &[Post]) -> Markup {
    base(
        "tommy.isnt.online",
        None,
        html! {
            div.content-panel {
                @for post in posts {
                    h2 {
                        a href=(format!("/post/{}", post.slug)) { (post.title) }
                    }
                    p.posted { (post.date.format("%Y-%m-%d").to_string()) }
                }
            }
        },
    )
}

/// Post page with pre-rendered content (run cells already swapped in).
pub fn post_with(post: &Post, content: &str, hits: u64, viewers: usize) -> Markup {
    base(
        &post.title,
        Some(&post.slug),
        html! {
            h1.title { (post.title) }
            div.content-panel {
                p.byline {
                    (post.date.format("%Y-%m-%d").to_string())
                    " · "
                    (hits) " views"
                    " · "
                    span #viewer-count { (viewers) " reading now" }
                }
                (PreEscaped(content))
            }
            script src="/static/cursor.js" {}
        },
    )
}

/// One `<details>` per baked run: model slug in the summary,
// expanding into the run's raw HTML, unescaped.
pub fn runcell(hole: &str, runs: &[crate::runs::Run]) -> Markup {
    html! {
        div.runcell data-hole=(hole) {
            @for run in runs {
                details {
                    summary { (run.model) }
                    div.run-body { (PreEscaped(run.html.clone())) }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(model: &str, html: &str) -> crate::runs::Run {
        crate::runs::Run {
            model: model.to_string(),
            html: html.to_string(),
        }
    }

    #[test]
    fn one_details_per_run_raw_html() {
        let out = runcell(
            "h1",
            &[run("a", "<p>A</p>"), run("b", "<em>B</em>")],
        )
        .into_string();
        assert_eq!(out.matches("<details>").count(), 2);
        assert!(out.contains("<summary>a</summary>"));
        assert!(out.contains("<summary>b</summary>"));
        // Raw, not escaped.
        assert!(out.contains("<p>A</p>"));
        assert!(!out.contains("&lt;p&gt;"));
        assert!(out.contains("data-hole=\"h1\""));
    }
}

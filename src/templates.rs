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

pub fn post(post: &Post, hits: u64, viewers: usize) -> Markup {
    post_with(post, &post.content, hits, viewers)
}

/// Post page with pre-rendered content (run cells already swapped in).
/// `post()` is the plain path; this is the jukebox path.
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

/// One swappable AI voice: model picker form (GET, bookmarkable) +
/// the selected run's paragraphs, each in its own collapsible
/// `<details>`. No JS, no keys, no generation — the files are baked.
/// `selected` falls back to the first model when the query names
/// nothing or names a voice with no file.
pub fn runcell(slug: &str, hole: &str, models: &[String], selected: &crate::runs::Run) -> Markup {
    html! {
        div.runcell data-hole=(hole) {
            form method="get" action=(format!("/post/{slug}")) class="run-picker" {
                label { "voice: " }
                @for m in models {
                    @let active = m == &selected.model;
                    button type="submit" name=(format!("run_{hole}"))
                        value=(m) class=(if active { "model-chip active" } else { "model-chip" }) {
                        (m)
                    }
                }
            }
            details.open {
                summary { (format!("{} says:", selected.model)) }
                @for (i, para) in selected.paragraphs.iter().enumerate() {
                    details.run-para open {
                        summary { (format!("¶{}", i + 1)) }
                        p { (para) }
                    }
                }
            }
        }
    }
}

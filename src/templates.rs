use crate::posts::Post;
use maud::{html, Markup, PreEscaped, DOCTYPE};

fn base(title: &str, slug: Option<&str>, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
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
    base("tommy.isnt.online", None, html! {
        div.content-panel {
            @for post in posts {
                h2 {
                    a href=(format!("/post/{}", post.slug)) { (post.title) }
                }
                p.posted { (post.date.format("%Y-%m-%d").to_string()) }
            }
        }
    })
}

pub fn post(post: &Post, hits: u64, viewers: usize) -> Markup {
    base(&post.title, Some(&post.slug), html! {
        h1.title { (post.title) }
        div.content-panel {
            p.byline {
                (post.date.format("%Y-%m-%d").to_string())
                " · "
                (hits) " views"
                " · "
                span #viewer-count { (viewers) " reading now" }
            }
            (PreEscaped(&post.content))
        }
        script src="/static/cursor.js" {}
    })
}

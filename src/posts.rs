use chrono::NaiveDate;
use std::fs;

#[derive(Clone)]
pub struct Post {
    pub title: String,
    pub content: String,
    pub date: NaiveDate,
    pub slug: String,
}

pub fn load() -> Vec<Post> {
    let mut posts: Vec<Post> = fs::read_dir("posts")
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map_or(false, |n| n.ends_with(".html"))
        })
        .filter_map(|e| {
            let filename = e.file_name().into_string().ok()?;
            let content = fs::read_to_string(e.path()).ok()?;
            let (title_line, body) = content.split_once('\n')?;
            let title = title_line.trim().to_string();
            let date_str = filename.split('_').next()?;
            let date = NaiveDate::parse_from_str(date_str, "%Y%m%d").ok()?;
            let slug = slugify(&title);
            Some(Post { title, content: body.to_string(), date, slug })
        })
        .collect();

    posts.sort_by(|a, b| b.date.cmp(&a.date));
    posts
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

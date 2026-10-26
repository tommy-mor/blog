mod counter;
mod posts;
mod presence;
mod runs;
mod templates;

use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::services::ServeDir;

pub struct AppState {
    pub rooms: presence::Rooms,
    pub db: counter::Db,
}

pub type SharedState = Arc<AppState>;

async fn index(State(_state): State<SharedState>) -> impl IntoResponse {
    templates::index(&posts::load())
}

async fn post_page(
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    match posts::load().into_iter().find(|p| p.slug == slug) {
        Some(post) => {
            counter::increment(&state.db, &slug);
            let hits = counter::get_hits(&state.db, &slug);
            let viewers = state.rooms.viewer_count(&slug);
            // Swappable voices: `?run_<cell>=<model>` picks the file.
            // Each `<!-- run:<cell> -->` slot renders the selected run's
            // paragraphs (default: first model). Bookmarkable, no JS.
            let mut content = post.content.clone();
            // Holes are positional: h1 is the 1st marker in the narrative,
            // h2 the 2nd. Replace one occurrence at a time, in order, so
            // each voice lands in its own hole. Bare holes (no baked
            // runs) stay as-is — the narrative flows past them.
            let holes = runs::holes_for(&post.run_key, &post.content);
            for hole in runs::hole_ids(&post.content).iter() {
                let Some(models) = holes.get(hole) else {
                    continue;
                };
                let key = format!("run_{hole}");
                let want = query.get(&key).map(|s| s.as_str()).unwrap_or("");
                let model = if models.iter().any(|m| m == want) {
                    want.to_string()
                } else {
                    models[0].clone()
                };
                if let Some(run) = runs::load(&post.run_key, &hole, &model) {
                    let html = templates::runcell(&slug, &hole, &models, &run).into_string();
                    content = content.replacen(crate::runs::HOLE, &html, 1);
                }
            }
            templates::post_with(&post, &content, hits, viewers).into_response()
        }
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

async fn ws_handler(
    Path(slug): Path<String>,
    State(state): State<SharedState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| presence::handle(socket, slug, state))
}

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState {
        rooms: presence::Rooms::default(),
        db: counter::open("data/counter.redb"),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/post/:slug", get(post_page))
        .route("/ws/:slug", get(ws_handler))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    println!("listening on 0.0.0.0:8080");
    axum::serve(listener, app).await.unwrap();
}

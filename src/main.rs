mod counter;
mod posts;
mod presence;
mod runs;
mod templates;

use axum::{
    extract::{Path, State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
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

async fn post_page(Path(slug): Path<String>, State(state): State<SharedState>) -> impl IntoResponse {
    match posts::load().into_iter().find(|p| p.slug == slug) {
        Some(post) => {
            counter::increment(&state.db, &slug);
            let hits = counter::get_hits(&state.db, &slug);
            let viewers = state.rooms.viewer_count(&slug);
            // Each hole renders every baked run, one <details> per model.
            let mut content = post.content.clone();
            for hole in runs::hole_ids(&post.content) {
                let baked = runs::loads(&post.run_key, &hole);
                if baked.is_empty() {
                    continue;
                }
                let html = templates::runcell(&hole, &baked).into_string();
                content = content.replacen(crate::runs::HOLE, &html, 1);
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

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("listening on {addr}");
    axum::serve(listener, app).await.unwrap();
}

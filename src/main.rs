mod counter;
mod posts;
mod presence;
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

async fn post_page(
    Path(slug): Path<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    match posts::load().into_iter().find(|p| p.slug == slug) {
        Some(post) => {
            counter::increment(&state.db, &slug);
            let hits = counter::get_hits(&state.db, &slug);
            let viewers = state.rooms.viewer_count(&slug);
            templates::post(&post, hits, viewers).into_response()
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
        db: counter::open("data"),
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

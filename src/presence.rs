use crate::SharedState;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use uuid::Uuid;

type Tx = mpsc::UnboundedSender<String>;
type RoomMap = HashMap<String, HashMap<Uuid, Tx>>;

#[derive(Default, Clone)]
pub struct Rooms(Arc<RwLock<RoomMap>>);

impl Rooms {
    pub fn viewer_count(&self, slug: &str) -> usize {
        self.0.read().unwrap().get(slug).map_or(0, |r| r.len())
    }

    fn join(&self, slug: &str, id: Uuid, tx: Tx) {
        self.0
            .write()
            .unwrap()
            .entry(slug.to_string())
            .or_default()
            .insert(id, tx);
    }

    fn leave(&self, slug: &str, id: Uuid) {
        let mut rooms = self.0.write().unwrap();
        if let Some(room) = rooms.get_mut(slug) {
            room.remove(&id);
        }
    }

    fn broadcast_except(&self, slug: &str, sender: Uuid, msg: &str) {
        let rooms = self.0.read().unwrap();
        if let Some(room) = rooms.get(slug) {
            for (id, tx) in room {
                if *id != sender {
                    let _ = tx.send(msg.to_string());
                }
            }
        }
    }

    fn broadcast_all(&self, slug: &str, msg: &str) {
        let rooms = self.0.read().unwrap();
        if let Some(room) = rooms.get(slug) {
            for tx in room.values() {
                let _ = tx.send(msg.to_string());
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Cursor { block: u32, offset: u32, dx: f32, dy: f32 },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMsg {
    Cursor { id: Uuid, block: u32, offset: u32, dx: f32, dy: f32 },
    Leave { id: Uuid },
    Count { n: usize },
}

pub async fn handle(socket: WebSocket, slug: String, state: SharedState) {
    let id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    state.rooms.join(&slug, id, tx);

    let count_msg = serde_json::to_string(&ServerMsg::Count {
        n: state.rooms.viewer_count(&slug),
    })
    .unwrap();
    state.rooms.broadcast_all(&slug, &count_msg);

    let (mut sink, mut stream) = socket.split();

    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let rooms = state.rooms.clone();
    let slug2 = slug.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = stream.next().await {
            if let Ok(ClientMsg::Cursor { block, offset, dx, dy }) = serde_json::from_str(&text) {
                let out = serde_json::to_string(&ServerMsg::Cursor { id, block, offset, dx, dy })
                    .unwrap();
                rooms.broadcast_except(&slug2, id, &out);
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    state.rooms.leave(&slug, id);

    let leave_msg = serde_json::to_string(&ServerMsg::Leave { id }).unwrap();
    state.rooms.broadcast_except(&slug, id, &leave_msg);

    let count_msg = serde_json::to_string(&ServerMsg::Count {
        n: state.rooms.viewer_count(&slug),
    })
    .unwrap();
    state.rooms.broadcast_all(&slug, &count_msg);
}

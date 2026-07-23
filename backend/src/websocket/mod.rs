mod ticket;
mod types;

use std::sync::Arc;

use axum::extract::ws::Message;
use dashmap::DashMap;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::errors::AppError;

pub use ticket::{generate as generate_ticket, verify as verify_ticket, WebSocketTicketClaims};
pub use types::WebSocketEnvelope;

type SocketSender = UnboundedSender<Message>;

#[derive(Debug, Clone)]
struct SocketEntry {
    connection_id: Uuid,
    tx: SocketSender,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionKey {
    pub user_id: i32,
    pub connection_id: Uuid,
}

#[derive(Debug, Clone, Copy)]
pub struct HubConfig {
    pub max_connections_per_user: usize,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            max_connections_per_user: 8,
        }
    }
}

#[derive(Clone, Default)]
pub struct WebSocketHub {
    connections: Arc<DashMap<i32, Vec<SocketEntry>>>,
    config: HubConfig,
}

/* [174A-69] Hub websocket reusable del framework Glory.
 * El framework solo administra conexiones por usuario y fanout en memoria; el
 * upgrade HTTP y el bridge multi-instancia se montan encima en tareas posteriores. */

impl WebSocketHub {
    #[must_use]
    pub fn new(config: HubConfig) -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
            config,
        }
    }

    pub fn register(
        &self,
        user_id: i32,
        tx: UnboundedSender<Message>,
    ) -> Result<ConnectionKey, AppError> {
        let connection_id = Uuid::new_v4();
        let mut bucket = self.connections.entry(user_id).or_default();
        if self.config.max_connections_per_user > 0
            && bucket.len() >= self.config.max_connections_per_user
        {
            return Err(AppError::TooManyRequests(format!(
                "usuario {user_id} superó el máximo de conexiones websocket"
            )));
        }

        bucket.push(SocketEntry { connection_id, tx });
        Ok(ConnectionKey {
            user_id,
            connection_id,
        })
    }

    pub fn unregister(&self, key: ConnectionKey) {
        let mut remove_user = false;
        if let Some(mut bucket) = self.connections.get_mut(&key.user_id) {
            bucket.retain(|entry| entry.connection_id != key.connection_id);
            remove_user = bucket.is_empty();
        }
        if remove_user {
            let _ = self.connections.remove(&key.user_id);
        }
    }

    pub fn broadcast_user(
        &self,
        user_id: i32,
        envelope: &WebSocketEnvelope,
    ) -> Result<usize, AppError> {
        let payload = envelope.to_json()?;
        let mut delivered = 0usize;
        let mut remove_user = false;

        if let Some(mut bucket) = self.connections.get_mut(&user_id) {
            bucket.retain(|entry| {
                let send_result = entry.tx.send(Message::Text(payload.clone()));
                if send_result.is_ok() {
                    delivered += 1;
                    true
                } else {
                    false
                }
            });
            remove_user = bucket.is_empty();
        }

        if remove_user {
            let _ = self.connections.remove(&user_id);
        }

        Ok(delivered)
    }

    #[must_use]
    pub fn connection_count(&self, user_id: i32) -> usize {
        self.connections
            .get(&user_id)
            .map_or(0, |bucket| bucket.len())
    }

    #[must_use]
    pub fn total_users(&self) -> usize {
        self.connections.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{HubConfig, WebSocketEnvelope, WebSocketHub};
    use axum::extract::ws::Message;
    use serde_json::json;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn register_and_unregister_connection() {
        let hub = WebSocketHub::new(HubConfig::default());
        let (tx, _rx) = unbounded_channel::<Message>();
        let key = hub.register(7, tx).expect("register");

        assert_eq!(hub.connection_count(7), 1);
        assert_eq!(hub.total_users(), 1);

        hub.unregister(key);
        assert_eq!(hub.connection_count(7), 0);
        assert_eq!(hub.total_users(), 0);
    }

    #[test]
    fn enforces_max_connections_per_user() {
        let hub = WebSocketHub::new(HubConfig {
            max_connections_per_user: 1,
        });
        let (tx_one, _rx_one) = unbounded_channel::<Message>();
        let (tx_two, _rx_two) = unbounded_channel::<Message>();
        let _ = hub.register(9, tx_one).expect("first connection");

        assert!(hub.register(9, tx_two).is_err());
    }

    #[test]
    fn broadcast_prunes_closed_connections() {
        let hub = WebSocketHub::new(HubConfig::default());
        let (tx_live, mut rx_live) = unbounded_channel::<Message>();
        let (tx_dead, rx_dead) = unbounded_channel::<Message>();
        drop(rx_dead);

        let _live_key = hub.register(5, tx_live).expect("live connection");
        let _dead_key = hub.register(5, tx_dead).expect("dead connection");
        let delivered = hub
            .broadcast_user(
                5,
                &WebSocketEnvelope::Event {
                    name: "notification".into(),
                    payload: json!({ "id": 1 }),
                },
            )
            .expect("broadcast");

        assert_eq!(delivered, 1);
        assert_eq!(hub.connection_count(5), 1);
        let received = rx_live.try_recv().expect("received payload");
        assert!(matches!(received, Message::Text(_)));
    }
}

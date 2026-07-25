use axum::extract::ws::Message;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::errors::AppError;

/* [174A-69] Tipos base serializables para el hub websocket reusable.
 * El hub trabaja con envelopes genéricos para que cada app proyecte eventos
 * de dominio sin acoplar el framework a mensajes concretos de negocio. */

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebSocketEnvelope {
    Ping,
    Pong,
    Authenticated {
        user_id: i32,
        connection_id: Uuid,
    },
    Event {
        name: String,
        payload: serde_json::Value,
    },
    Error {
        code: String,
        message: String,
    },
}

impl WebSocketEnvelope {
    pub fn to_json(&self) -> Result<String, AppError> {
        serde_json::to_string(self)
            .map_err(|error| AppError::Internal(format!("serializar mensaje websocket: {error}")))
    }

    pub fn to_message(&self) -> Result<Message, AppError> {
        Ok(Message::Text(self.to_json()?))
    }
}

#[cfg(test)]
mod tests {
    use super::WebSocketEnvelope;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn serializes_event_envelope() {
        let message = WebSocketEnvelope::Event {
            name: "message_created".into(),
            payload: json!({ "id": 42 }),
        }
        .to_json()
        .expect("json");

        assert!(message.contains("message_created"));
        assert!(message.contains("\"id\":42"));
    }

    #[test]
    fn builds_authenticated_message() {
        let envelope = WebSocketEnvelope::Authenticated {
            user_id: 7,
            connection_id: Uuid::nil(),
        };
        let message = envelope.to_message().expect("message");
        assert!(matches!(message, axum::extract::ws::Message::Text(_)));
    }
}

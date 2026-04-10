/* Glory Backend — Error handling genérico.
 * AppError mapea cada variante a un HTTP status code + JSON response.
 * ErrorResponse es el formato público de error en la API. */

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

/// Tipos de error de la aplicación — cada variante mapea a un HTTP status code
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("No encontrado: {0}")]
    NotFound(String),

    #[error("Solicitud inválida: {0}")]
    BadRequest(String),

    #[error("No autorizado")]
    Unauthorized,

    #[error("Prohibido: {0}")]
    Forbidden(String),

    #[error("Conflicto: {0}")]
    Conflict(String),

    #[error("Error interno: {0}")]
    Internal(String),

    #[error("Error de base de datos: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Error de validación: {0}")]
    Validation(String),

    /* [104A-36] Rate limiting — HTTP 429 Too Many Requests */
    #[error("Demasiadas solicitudes: {0}")]
    TooManyRequests(String),
}

/// Estructura de respuesta de error expuesta en la API
#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Tipo de error (`not_found`, `unauthorized`, etc.)
    pub error: String,
    /// Mensaje legible para el usuario
    pub message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match &self {
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg.clone()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg.clone()),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Credenciales inválidas o ausentes".to_string(),
            ),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg.clone()),
            Self::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg.clone()),
            Self::Internal(msg) => {
                tracing::error!("Error interno: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Ocurrió un error interno".to_string(),
                )
            }
            Self::Database(err) => {
                tracing::error!("Error de base de datos: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Ocurrió un error de base de datos".to_string(),
                )
            }
            Self::Validation(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_error",
                msg.clone(),
            ),
            Self::TooManyRequests(msg) => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_requests",
                msg.clone(),
            ),
        };

        let body = ErrorResponse {
            error: error_type.to_string(),
            message,
        };

        (status, Json(body)).into_response()
    }
}

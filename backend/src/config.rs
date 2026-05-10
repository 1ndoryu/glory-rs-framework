/* Glory Backend — Configuración desde variables de entorno.
 * AppConfig carga DATABASE_URL, JWT_SECRET, host/port y SMTP opcional.
 * SmtpConfig es opcional — si las variables SMTP no están, el sistema
 * funciona sin email (loguea enlaces en vez de enviarlos). */

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Variable de entorno requerida no encontrada: {0}")]
    MissingEnvVar(String),
    #[error("Puerto inválido: {0}")]
    InvalidPort(#[from] std::num::ParseIntError),
}

/// Configuración de la aplicación cargada desde variables de entorno
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub jwt_secret: String,
    pub host: String,
    pub port: u16,
    pub smtp: Option<SmtpConfig>,
    pub app_url: String,
    /// Email de destino para reportes de errores (opcional)
    pub error_report_email: Option<String>,
}

/// Configuración SMTP para envío de emails
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub from_email: String,
    pub from_name: String,
}

impl AppConfig {
    /// Carga la configuración desde variables de entorno.
    /// Requiere `DATABASE_URL` y `JWT_SECRET`. `HOST` y `PORT` son opcionales.
    pub fn from_env() -> Result<Self, ConfigError> {
        let smtp = Self::load_smtp();
        Ok(Self {
            database_url: std::env::var("DATABASE_URL")
                .map_err(|_| ConfigError::MissingEnvVar("DATABASE_URL".into()))?,
            jwt_secret: std::env::var("JWT_SECRET")
                .map_err(|_| ConfigError::MissingEnvVar("JWT_SECRET".into()))?,
            /* [105A-1] Default Docker-safe: si una app escucha solo en loopback,
             * el health local puede pasar mientras Traefik no alcanza la IP del contenedor. */
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()?,
            smtp,
            app_url: std::env::var("APP_URL")
                .unwrap_or_else(|_| "http://localhost:5173".to_string()),
            error_report_email: std::env::var("ERROR_REPORT_EMAIL").ok(),
        })
    }

    fn load_smtp() -> Option<SmtpConfig> {
        let host = std::env::var("SMTP_HOST").ok()?;
        let port = std::env::var("SMTP_PORT").ok()?.parse().ok()?;
        let user = std::env::var("SMTP_USER").ok()?;
        let password = std::env::var("SMTP_PASSWORD").ok()?;
        Some(SmtpConfig {
            host,
            port,
            user,
            password,
            from_email: std::env::var("SMTP_FROM_EMAIL")
                .unwrap_or_else(|_| "noreply@app.com".to_string()),
            from_name: std::env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "App".to_string()),
        })
    }
}

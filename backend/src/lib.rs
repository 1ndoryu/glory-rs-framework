/* Glory Backend Framework — Módulos fundacionales reutilizables.
 * Provee error handling, configuración, y patrones base para web apps
 * con Axum + SQLx + PostgreSQL. */

#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod config;
pub mod errors;
pub mod fixtures;
pub mod runtime;
pub mod websocket;

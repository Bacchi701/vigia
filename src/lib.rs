//! Vigía: monitor local de Windows que **solo observa**.
//!
//! Mapa de módulos (ver también `ARQUITECTURA.md` y `GUIA_DE_USO.md`):
//! - [`win`]: único módulo con `unsafe`; llamadas directas a la API de Windows.
//! - [`colector`]: toma muestras periódicas y calcula tasas (CPU %, bytes/s).
//! - [`historial`]: muestras en vivo y resúmenes por minuto en SQLite.
//! - [`base`]: apertura y esquema de la base de datos.
//! - [`error`]: tipo de error común.
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod app;
pub mod avisos;
pub mod base;
pub mod cliente;
pub mod colector;
pub mod conexiones;
pub mod disco;
pub mod error;
pub mod exportar;
pub mod historial;
pub mod inicio;
pub mod servidor;
pub mod vigilante;
pub mod win;

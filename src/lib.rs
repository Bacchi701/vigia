//! Vigía: monitor local de Windows que **solo observa**.
//!
//! Mapa de módulos (ver también `ARQUITECTURA.md` y `GUIA_DE_USO.md`):
//! - [`win`]: único módulo con `unsafe`; llamadas directas a la API de Windows.
//! - [`colector`]: toma muestras periódicas y calcula tasas (CPU %, bytes/s).
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod colector;
pub mod win;

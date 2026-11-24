# Decisiones

Formato: decisión · alternativas · motivo.

## Dependencias (crates)

| Crate | Para qué |
|---|---|
| `windows` 0.62 | Llamadas directas a la API de Windows (procesos, memoria, discos, red, registro, archivos, aleatorio seguro, notificaciones, batería). Solo se activan las *features* que se usan. |
| `rusqlite` 0.43 (`bundled`) | Base de datos SQLite del historial. `bundled` compila SQLite dentro del ejecutable: no hace falta instalar nada. |
| `serde` + `serde_json` | Convertir estructuras a JSON para la API y guardar resultados de análisis de disco. |
| `tiny_http` 0.12 | Servidor HTTP pequeño y síncrono; basta para una sola persona en 127.0.0.1. |

No se usa `sysinfo` ni crates parecidos: las llamadas a Windows están escritas a mano en `src/win/`.

## Decisiones generales

- **Edición 2024 de Rust** · 2021 · es la que crea `cargo init` con Rust 1.100.
- **Reglas del compilador**: `#![deny(unsafe_code)]` en la raíz y `allow` solo en `src/win/`; `clippy::unwrap_used` prohibido fuera de pruebas · revisarlo a mano · así el compilador hace cumplir las reglas de estilo.

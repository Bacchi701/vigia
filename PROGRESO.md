# Progreso de Vigía

INICIO: 2026-11-24T05:02:05.2553851-03:00

## Hitos

| Hito | Estado | Cierre |
|---|---|---|
| H0 Preparación | terminado | 2026-11-24T05:07:23-03:00 |
| H1 Colector | terminado | 2026-11-24T05:15:03.6424647-03:00 |
| H2 Historial | pendiente | |
| H3 Interfaz | pendiente | |
| H4 Disco | pendiente | |
| H5 Inicio y conexiones | pendiente | |
| H6 Avisos | pendiente | |
| H7 Entrega | pendiente | |
| Extra 1 Resumen semanal | pendiente | |
| Extra 2 Exportar CSV | pendiente | |
| Extra 3 Notificaciones de Windows | pendiente | |
| Extra 4 Batería | pendiente | |

## Resultados de pruebas por hito

### H0
- `cargo build` y `cargo build --release` compilan con todas las dependencias (windows 0.62.2, rusqlite 0.43 bundled, serde, serde_json, tiny_http 0.12).
- Llamada mínima `GlobalMemoryStatusEx` funciona: imprimió «Memoria: 20292 MB usados de 32710 MB».
- `cargo fmt` 1.11 y `cargo clippy` 0.1.100 disponibles.

### H1
- `cargo test`: 11 pruebas pasan (7 unitarias + 4 de integración en `tests/colector.rs`).
  - El propio proceso aparece con ruta igual a `current_exe`, memoria > 0, CPU 0–100, hora de inicio coherente.
  - `vigia --prueba-cpu 8` se detecta con CPU > 50 % de un núcleo y entre los 10 que más CPU usan.
  - `vigia --prueba-memoria 300 8` se detecta con más de 250 MB.
  - PID 4 (System) aparece sin ruta y sin error; menos del 10 % de procesos sin ruta (en la prueba manual: 2 de ~370).
- `cargo clippy --all-targets -- -D warnings` sin avisos de código (solo los avisos de Cargo por dependencias aún sin usar).
- Tras las pruebas, `tasklist` no muestra ningún `vigia.exe` abierto.

## Qué sigue
H2 Historial.

## Problemas abiertos
Ninguno.

## Cómo reanudar
Leer este archivo, ver el primer hito no terminado en la tabla, revisar `git log` y continuar desde ahí.

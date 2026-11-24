# Progreso de Vigía

INICIO: 2026-11-24T05:02:05.2553851-03:00

## Hitos

| Hito | Estado | Cierre |
|---|---|---|
| H0 Preparación | terminado | 2026-11-24T05:07:23-03:00 |
| H1 Colector | pendiente | |
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

## Qué sigue
H1 Colector.

## Problemas abiertos
Ninguno.

## Cómo reanudar
Leer este archivo, ver el primer hito no terminado en la tabla, revisar `git log` y continuar desde ahí.

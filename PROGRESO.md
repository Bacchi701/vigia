# Progreso de Vigía

INICIO: 2026-11-24T05:02:05.2553851-03:00

## Hitos

| Hito | Estado | Cierre |
|---|---|---|
| H0 Preparación | terminado | 2026-11-24T05:07:23-03:00 |
| H1 Colector | terminado | 2026-11-24T05:15:03.6424647-03:00 |
| H2 Historial | terminado | 2026-11-24T05:18:59.5027222-03:00 |
| H3 Interfaz | terminado (revisión del subagente en curso) | 2026-11-24T05:28:45.3947719-03:00 |
| H4 Disco | terminado | 2026-11-24T05:35:34.5801355-03:00 |
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

### H2
- `cargo test`: 19 pruebas pasan (14 unitarias, 4 del colector, 1 de historial en disco).
  - Con reloj simulado: agrupación por app (chrome.exe + Chrome.exe = 1 app), cierre de minuto, capacidad del búfer en vivo, selección de las apps que más consumen + `(otros)` (la suma cuadra: 177 %), consultas `top_apps` / `momento` / `linea_tiempo` sobre 2 horas simuladas, retención por 30 días y por tope de tamaño (queda bajo el tope y nunca borra la última hora).
  - Con muestras reales: se guarda un minuto en `target/tmp/historial_prueba.db`, se reabre y se consulta.
- `cargo clippy --all-targets -- -D warnings` sin avisos de código.

### H3
- `cargo test`: 27 pruebas pasan (17 unitarias + 4 colector + 1 historial + 4 servidor + 1 arranque). Nuevas: `tests/servidor.rs` (4) y `tests/arranque.rs` (1), más unitarias de servidor y cliente.
  - La página carga con CSP y sin recursos externos; Host ajeno → 403, Origin ajeno → 403 (también para apagar), sin token o con token incorrecto → 401; Origin propio y `localhost` aceptados.
  - `/api/ahora` incluye el proceso de la prueba; `/api/proceso`, `/api/historial/*`, `/api/apps` responden; parámetro inválido → 400.
  - Arranque real de `vigia.exe --sin-navegador --datos ...`: escribe su archivo de estado, sirve la página, un segundo arranque detecta la instancia y sale, `--detener` la cierra y borra el archivo.
- Prueba manual con la versión release en el panel del navegador: vistas Ahora, detalle «¿Qué es esto?», Historial y Aplicaciones, en modo claro y oscuro, **sin errores en la consola de JavaScript**.
- `cargo clippy --all-targets -- -D warnings` sin avisos.

### H4
- `cargo test`: 35 pruebas pasan (21 unitarias + 1 arranque + 4 colector + 3 disco + 1 historial + 5 servidor).
  - Árbol generado en `target/tmp/arbol_disco` (6 archivos, 28 010 bytes, con una unión que apunta a la raíz): total, árbol, archivos grandes y tipos exactos; la unión se omite (1 enlace omitido, sin ciclo).
  - Se agrega un archivo de 7000 bytes y la comparación muestra la carpeta `a\b` con +7000.
  - Cancelar antes de empezar devuelve un análisis incompleto.
  - Carpeta del proyecto: se analiza completa (incluye `.rs` y `.toml`; `target` pesa más de 1 MB).
  - API: analizar, progreso, lista y resultado; ruta inválida → 400; «abrir» con ruta ajena o sin id → 400.
- Prueba manual en el navegador (release): análisis de `C:\Users\BRYAN\vigia` (1,3 GB, 2479 archivos, 1 s) y segundo análisis con comparación; sin errores de consola.
- `cargo clippy --all-targets -- -D warnings` sin avisos.

## Qué sigue
Aplicar la revisión de H3 cuando llegue. H5 Inicio y conexiones.

## Problemas abiertos
Ninguno.

## Cómo reanudar
Leer este archivo, ver el primer hito no terminado en la tabla, revisar `git log` y continuar desde ahí.

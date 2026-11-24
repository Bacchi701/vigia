# Progreso de Vigía

INICIO: 2026-11-24T05:02:05.2553851-03:00

## Hitos

| Hito | Estado | Cierre |
|---|---|---|
| H0 Preparación | terminado | 2026-11-24T05:07:23-03:00 |
| H1 Colector | terminado | 2026-11-24T05:15:03.6424647-03:00 |
| H2 Historial | terminado | 2026-11-24T05:18:59.5027222-03:00 |
| H3 Interfaz | terminado; revisión aplicada 2026-11-24T05:55:38.6190022-03:00 (ver REVISION_H3.md) | 2026-11-24T05:28:45.3947719-03:00 |
| H4 Disco | terminado | 2026-11-24T05:35:34.5801355-03:00 |
| H5 Inicio y conexiones | terminado | 2026-11-24T05:42:52.4085010-03:00 |
| H6 Avisos | terminado | 2026-11-24T06:00:47.7465406-03:00 |
| H7 Entrega | terminado | 2026-11-24T06:26:01.9141625-03:00 |
| Extra 1 Resumen semanal | terminado | 2026-11-24T06:26:01.9141625-03:00 |
| Extra 2 Exportar CSV | terminado | 2026-11-24T06:26:01.9141625-03:00 |
| Extra 3 Notificaciones de Windows | terminado (ventana propia; ver PREGUNTAS.md) | 2026-11-24T06:26:01.9141625-03:00 |
| Extra 4 Batería | terminado | 2026-11-24T06:26:01.9141625-03:00 |

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

### H5
- `cargo test`: 48 pruebas pasan (33 unitarias + 1 arranque + 4 colector + 3 disco + 1 historial + 6 servidor).
  - Lectura real del inicio (registro `Run`/`RunOnce` de HKCU, HKLM y WOW6432Node, más carpetas de inicio) sin fallar; extracción del ejecutable de comandos con comillas, sin comillas y con `%windir%`.
  - Comparación de revisiones: la primera solo guarda referencia; luego detecta entradas nuevas y quitadas.
  - Tablas TCP/UDP IPv4/IPv6: un `TcpListener` propio aparece en escucha con el PID y el nombre de proceso correctos.
  - Puertos nuevos: avisan una sola vez (si se cierra y reabre no repite); UDP no genera avisos.
  - API: `/api/inicio`, `/api/conexiones`, `POST /api/revisar` (un puerto abierto después de la referencia produce el aviso `puerto_nuevo`), `/api/avisos` y marcar vistos.
- Prueba manual (release): vistas Inicio, Conexiones y Avisos con datos reales. Se corrigió un error encontrado ahí: un puerto en la IP de una interfaz (no 0.0.0.0) aparecía como «solo este equipo».
- `cargo clippy --all-targets -- -D warnings` sin avisos.

### Revisión de H3 (subagente)
- Se aplicaron 7 correcciones del backend, 8 de la interfaz y 5 de solidez del `unsafe` (detalle en `REVISION_H3.md`). Pruebas nuevas: archivo que realmente se achica, minuto repetido que se combina, reloj que retrocede, análisis de disco que no borran historial, dos arranques simultáneos, parámetros extremos y cliente lento sin token. 57 pruebas pasan tras la revisión.
- Verificado en el navegador: fila agrupada abre el detalle, gráficos del detalle sin parpadeo, clic en el historial de 30 días, franja «Sin conexión» al detener Vigía.

### H6
- `cargo test`: 60 pruebas pasan (40 unitarias + 2 arranque + 4 colector + 3 disco + 2 historial + 9 servidor).
  - Con datos simulados: memoria al 95 % avisa solo después de 5 min seguidos (una baja reinicia la cuenta) y una sola vez; un proceso al 80 % avisa una vez en 30 min y nada si la regla está desactivada; poco disco avisa, no repite en 1 h y sí a las 7 h; reglas fuera de rango se corrigen.
  - Por la API: leer y guardar reglas, cuerpo inválido → 400; con umbral de disco al 99 % llega un aviso real `disco_poco`; con la regla de puertos desactivada no hay aviso de puerto nuevo.
- Prueba manual (release): formulario de reglas guardado desde la interfaz. Se agregó la opción «solo puertos accesibles desde otros equipos» porque aparecieron 29 avisos de puertos locales de herramientas de desarrollo.
- `cargo clippy --all-targets -- -D warnings` sin avisos.

### Extras (programados mientras corría la medición de consumo de H7)
- `cargo test`: 67 pruebas pasan (44 unitarias + 2 arranque + 4 colector + 3 disco + 2 historial + 1 notificación + 11 servidor).
  - Resumen semanal con datos simulados: aplicación con más CPU y con más memoria, dos picos separados más de una hora con la app culpable, período vacío sin errores; `/api/resumen` responde.
  - CSV: encabezados, BOM, coma decimal, campos con `;` y comillas bien escapados; `/api/exportar` entrega `text/csv` con nombre de archivo.
  - Notificación propia mostrada de verdad: 3 se muestran y se cierran solas en ~1,5 s, la cuarta no tiene lugar, y al cerrarse liberan su lugar.
  - Batería: la lectura no falla (este equipo no tiene batería, así que la tarjeta no se muestra).
  - Hora local con `SystemTimeToTzSpecificLocalTime`: formato correcto.
- `cargo clippy --all-targets -- -D warnings` sin avisos.

### H7
- Verificación final: `cargo fmt --check` sin cambios, `cargo clippy --all-targets -- -D warnings` sin avisos, `cargo test` 67 de 67, `cargo build --release` al día.
- `iniciar_vigia.bat` (ejecutado con `cmd /c`): Vigía arrancó, abrió el navegador y respondió en `/api/estado`. `detener_vigia.bat`: «Vigía se cerró.», sin procesos `vigia.exe` y sin archivo de estado.
- Las 8 pantallas cargan en el navegador (release) sin errores de JavaScript; CSV con hora local; casilla de notificaciones y botones de exportar presentes.
- Consumo medido (release, 6 procesadores, ~350 procesos):
  - Sin la página, 10 min, mientras se compilaba y corrían pruebas: CPU 0,11 %, *working set* 16,6 MB promedio (19,6 máx.), privada 6,8 MB (10,1 máx.).
  - Pidiendo `/api/ahora` cada 2 s como la pantalla Ahora, 5 min (134 peticiones de ~207 KB): CPU 0,036 %, *working set* 18,6 MB (20,2 máx.), privada 9,3 MB (11,3 máx.).
  - Un primer intento con la página en el panel del navegador no sirvió: el panel estaba oculto y frenó los temporizadores de la página (la última muestra mostrada tenía 2 min). Se repitió con un script que pide los datos igual que la página.
- Documentos: `README.md`, `GUIA_DE_USO.md`, `ARQUITECTURA.md` actualizado, `PREGUNTAS.md`, `INFORME_FINAL.md`.
- Revisión final (sugerida por el asesor):
  - Se completaron los comentarios de las llamadas a Windows en `src/win/notificacion.rs`.
  - Se tomaron capturas de Inicio y Conexiones en modo claro, y de Semana, Disco, Inicio, Conexiones y Avisos en oscuro.
  - Se corrigió el encabezado: con 8 pestañas, el menú pasa a su propia fila en pantallas de menos de 1180 px.
  - Se precisó el informe: el extra 3 es un sustituto, las escrituras en el registro de las notificaciones nativas no se verificaron, y quedaron archivos auxiliares en la carpeta temporal de la sesión.
  - Los scripts de medición se copiaron a `herramientas/`.
  - Tras estos cambios, `cargo fmt --check`, clippy (0 avisos), `cargo test` (67 de 67) y `cargo build --release` pasaron, y no queda ningún `vigia.exe` en marcha.

## Qué sigue
Nada del plan. Todo terminado; ver `INFORME_FINAL.md`.

## Problemas abiertos
Ver «Problemas pendientes y cosas no verificadas» en `INFORME_FINAL.md` (nada bloquea el uso).

## Cómo reanudar
Leer este archivo, ver el primer hito no terminado en la tabla, revisar `git log` y continuar desde ahí.

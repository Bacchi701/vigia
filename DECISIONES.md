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

## H1 · Colector

- **Lista de procesos con `NtQuerySystemInformation(SystemProcessInformation)`** · Toolhelp + `OpenProcess` por proceso · con `OpenProcess`, sin administrador, 146 de 370 procesos no dejaban leer CPU ni memoria; la llamada de ntdll entrega todo de todos en una sola llamada (es lo que usa el Administrador de tareas). Los campos "reservados" se leen con el diseño documentado por System Informer; está comentado en `src/win/proceso.rs`.
- **Ruta del ejecutable**: primero `OpenProcess` + `QueryFullProcessImageNameW`; si falla, `SystemProcessIdInformation` (clase 88) + `QueryDosDeviceW` · dejar la ruta vacía · así solo quedan sin ruta 2 procesos (System y otro protegido).
- **«Memoria» = memoria física privada** (*private working set*) · *working set* total o memoria comprometida · es la columna «Memoria» del Administrador de tareas; las otras dos también se envían y se muestran en el detalle.
- **CPU por proceso en % del equipo completo (0–100)** · % de un núcleo (0–100×n) · igual que el Administrador de tareas.
- **Ruta, descripción y empresa en caché por (PID, hora de inicio)**, y la información de versión por ruta · leerlas en cada muestra · no cambian mientras vive el proceso y leer el recurso de versión implica abrir el archivo. El PID solo no basta porque Windows reutiliza PIDs.
- **Adaptadores de red**: solo interfaces de hardware que no sean "de filtro" (bits de `InterfaceAndOperStatusFlags`) · todas · las de filtro repiten los bytes del adaptador real.
- **Unidades**: solo fijas y extraíbles, consultadas cada 30 s · incluir unidades de red · consultar una unidad de red caída puede colgar el colector.
- **E/S por proceso** = bytes de lectura/escritura de `IO_COUNTERS`, que mezclan disco, red y dispositivos · separarlos requiere ETW y administrador.
- **Modos de prueba en el mismo ejecutable** (`--prueba-cpu S`, `--prueba-memoria MB S`) · un binario aparte · las pruebas lanzan `vigia.exe` en ese modo, que termina solo, y además lo cierran en `Drop`.

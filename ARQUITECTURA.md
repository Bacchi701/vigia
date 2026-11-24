# Arquitectura de Vigía

Un solo ejecutable (`vigia.exe`) con varios hilos que comparten estado (`Arc<Estado>` con `Mutex` dentro):

```
                         API de Windows
                               │
              ┌────────────────┴────────────────┐
              │  src/win/  (único código unsafe) │
              └───┬───────────┬───────────┬──────┘
                  │           │           │
   hilo colector  │   hilo vigilante      │  hilo disco (bajo demanda)
   cada 2 s       │   cada 60 s           │  disco.rs
   colector.rs ───┤   inicio.rs           │
        │         │   conexiones.rs       │
        ▼         │   vigilante.rs        │
   historial.rs   │        │              │
   avisos.rs      │        ▼              ▼
   (motor)        └──► SQLite: datos/vigia.db  (base.rs)
        │                   ▲
        ▼                   │
   notificación propia      │
   (win/notificacion.rs)    │
                            │
   hilo servidor ── servidor.rs (un hilo por petición, máx. 16)
   solo 127.0.0.1           │  Host/Origin/token
                            ▼
                  ui/index.html (incluida en el .exe)
```

## Módulos

| Archivo | Qué hace |
|---|---|
| `src/win/` | Único lugar con `unsafe`. Cada función pública es segura y devuelve `Result` o `Option`. |
| `src/colector.rs` | Convierte contadores acumulados de Windows en tasas (CPU %, bytes/s). |
| `src/historial.rs` | Muestras en vivo (memoria), resumen por minuto y por aplicación (SQLite), retención, consultas y resumen semanal. |
| `src/base.rs` | Abre la base, crea el esquema, configuración clave/valor. |
| `src/disco.rs` | Análisis de carpetas, árbol resumido, comparación entre análisis. |
| `src/inicio.rs` | Programas de inicio (registro y carpetas). |
| `src/conexiones.rs` | Conexiones por proceso y puertos en escucha. |
| `src/vigilante.rs` | Compara inicio y puertos con la revisión anterior. |
| `src/avisos.rs` | Reglas, motor que las evalúa en cada muestra y registro de avisos. |
| `src/exportar.rs` | CSV para Excel. |
| `src/app.rs` | Arranca y detiene los hilos; estado compartido. |
| `src/servidor.rs` | API JSON y página; seguridad. |
| `src/cliente.rs` | Cliente HTTP mínimo (para `--detener` y pruebas). |
| `src/main.rs` | Línea de comandos, instancia única. |
| `ui/index.html` | Interfaz: HTML, CSS y JavaScript sin dependencias. |

## Principios

- **Solo observa.** Ninguna llamada modifica el sistema: el registro se abre con `KEY_READ`, los procesos con `PROCESS_QUERY_LIMITED_INFORMATION`, los archivos solo se listan.
- **La hora es un parámetro.** Historial, avisos y exportación reciben la hora (`ts`, `ahora_ms`); las pruebas simulan horas y días con muestras falsas.
- **Sin administrador.** Todo dato que Windows niega queda como `None` y se muestra como «No disponible»; nunca es un error.
- **Liviano.** Una sola llamada (`NtQuerySystemInformation`) trae todos los procesos; ruta y versión de cada ejecutable se leen una vez y quedan en caché; las unidades se consultan cada 30 s.

# Informe final de Vigía

**La aplicación funciona.** Para abrirla, haz doble clic en `iniciar_vigia.bat` (en `C:\Users\BRYAN\vigia`). Se abre en el navegador y queda una ventana minimizada llamada «Vigia». Para cerrarla, doble clic en `detener_vigia.bat`. Los dos archivos se probaron al final del trabajo: el primero levantó Vigía y abrió el navegador; el segundo lo cerró ordenadamente.

La versión release ya está compilada (`target\release\vigia.exe`, 3 MB). Si cambias el código, compila con `cargo build --release` con Vigía cerrado.

## Qué se construyó

Un solo ejecutable en Rust que toma una muestra del sistema cada 2 segundos, guarda un resumen por minuto en SQLite (`datos\vigia.db`) y sirve una página web en `127.0.0.1`. La página tiene ocho pantallas:

| Pantalla | Qué muestra |
|---|---|
| Ahora | Procesos con CPU, memoria y lectura/escritura en vivo (ordenable, filtrable, agrupable por aplicación), gráficos de los últimos 10 minutos, unidades, red y batería (si hay). Al hacer clic en un proceso se abre «¿Qué es esto?»: ruta, empresa, descripción, quién lo inició, hora de inicio y sus gráficos. |
| Historial | Línea de tiempo de 1 hora a 30 días. Al hacer clic en un momento se ve qué aplicaciones consumían más CPU y memoria entonces. Exportación a CSV para Excel. |
| Aplicaciones | Uso por aplicación hoy, ayer, en 7 y en 30 días. |
| Semana | Resumen de los últimos 7 días: aplicaciones que más consumieron, los 5 momentos de mayor carga (con la aplicación responsable) y la carga por día. |
| Disco | Análisis bajo demanda de una carpeta o unidad: árbol de carpetas, 100 archivos más grandes, espacio por tipo, comparación con el análisis anterior. Se puede cancelar. Nunca borra; solo ofrece abrir la ubicación en el Explorador. |
| Inicio | Programas que arrancan con Windows (claves `Run`/`RunOnce` y carpetas de inicio), con aviso cuando aparece o desaparece uno. |
| Conexiones | Puertos en escucha (y si otros equipos podrían conectarse) y conexiones TCP activas por programa. |
| Avisos | Reglas configurables y lista de avisos: memoria alta sostenida, un proceso con CPU alta sostenida, poco espacio en disco, inicio nuevo y puerto nuevo. Opcionalmente, una notificación en la esquina de la pantalla. |

Toda la información se pide directamente a la API de Windows con el crate `windows`, sin bibliotecas que lo resuelvan todo. El código `unsafe` está solo en `src/win/` y cada llamada tiene un comentario que explica qué función es, qué devuelve y por qué se eligió. `GUIA_DE_USO.md` incluye un mapa del código con el orden en que conviene leerlo.

Seguridad: el servidor escucha solo en `127.0.0.1`. Rechaza peticiones cuyo `Host` u `Origin` no sean los suyos y exige en la API un token aleatorio de 256 bits que se genera en cada arranque. La página no carga nada de internet (lo impide su política de contenido). Nunca hay dos Vigías a la vez, ni siquiera si se abre dos veces seguidas.

## Qué no se puede obtener sin permisos de administrador

- **Ruta del ejecutable de 2 procesos protegidos**: `System` y `Secure System`. De los otros 350 procesos de este equipo sí se obtiene. Primero se intenta abriendo el proceso; si Windows lo niega, se pide la ruta al núcleo sin abrirlo.
- **CPU, memoria y lectura/escritura de todos los procesos** sí se obtienen sin administrador. Se usa la misma llamada que el Administrador de tareas: `NtQuerySystemInformation`.
- **Lectura/escritura por proceso separada en disco y red**: Windows solo da el total. Separarlo requiere seguimiento de eventos (ETW) con administrador.
- **Carpetas de otros usuarios y del sistema** (por ejemplo `System Volume Information`): el análisis de disco las cuenta como «sin permiso» y sigue.
- Las tablas de conexiones con el PID de cada una sí se leen completas sin administrador.

## Resultados de las pruebas

Última ejecución completa (`cargo test`): **67 pruebas, todas pasan**. `cargo clippy --all-targets -- -D warnings` sin avisos y `cargo fmt --check` sin cambios.

| Grupo | Pruebas | Qué comprueban (resumen) |
|---|---|---|
| Unitarias | 44 | Cálculos de CPU y tasas; historial con reloj simulado (agrupación por aplicación, cierre de minutos, retención por días y por tamaño, minuto repetido que se combina, reloj que retrocede); disco (comparación, rutas permitidas); inicio (extracción del ejecutable, comparación); conexiones (alcance de direcciones, puertos nuevos); motor de avisos con datos simulados; CSV; utilidades de Windows. |
| Colector | 4 | El propio proceso aparece con valores coherentes; un proceso de prueba que ocupa un núcleo y otro que reserva 300 MB se detectan; `System` aparece sin ruta y sin error. |
| Disco | 3 | Árbol generado con tamaños exactos y una unión que apunta a sí misma (se salta, sin ciclo); comparación tras agregar un archivo; cancelación; análisis de la carpeta del proyecto. |
| Historial | 2 | Muestras reales guardadas y consultadas tras reabrir la base; el archivo de la base realmente se achica tras la retención. |
| Servidor | 11 | Página con cabeceras de seguridad; rechazo por `Host`, `Origin` y token; todas las rutas de la API; parámetros extremos; un cliente lento sin token no bloquea; análisis de disco, reglas, avisos, resumen y CSV por la API. |
| Arranque | 2 | `vigia.exe` real: arranca, responde, un segundo arranque no crea otra instancia (también si se abren dos a la vez) y `--detener` lo cierra. |
| Notificación | 1 | Se muestran 3 notificaciones reales, se cierran solas y la cuarta no tiene lugar. |

Además se revisó a mano en el navegador cada pantalla, en modo claro y oscuro, sin errores en la consola de JavaScript.

Después de H3, un revisor independiente revisó el colector, el historial y la interfaz, y encontró 15 problemas reales y 5 mejoras de solidez. Todos se corrigieron, salvo dos puntos menores en que se explica por qué no (ver `REVISION_H3.md`). Los más importantes:

- La base no devolvía espacio al disco.
- Reiniciar Vigía dentro de un mismo minuto pisaba ese minuto.
- Se podían abrir dos instancias a la vez.
- Un cliente lento podía bloquear el servidor.

## Consumo medido de Vigía

Medido con la versión release en este equipo (6 procesadores lógicos, unos 350 procesos), leyendo el tiempo de CPU y la memoria del proceso `vigia.exe` cada 10 segundos:

| Situación | Duración | CPU (% del equipo) | Memoria (*working set*) promedio / máxima | Memoria privada promedio / máxima |
|---|---|---|---|---|
| Sin la página abierta, mientras yo compilaba y corría pruebas (muchos procesos nuevos) | 10 min | 0,11 % | 16,6 / 19,6 MB | 6,8 / 10,1 MB |
| Con la página pidiendo datos cada 2 s, como la pantalla «Ahora» (134 peticiones de ~207 KB) | 5 min | 0,036 % | 18,6 / 20,2 MB | 9,3 / 11,3 MB |

Las dos situaciones quedan muy por debajo de las metas (menos de 1 % de CPU y menos de 50 MB). Hay dos cosas que no medí:

- El consumo durante el análisis de una unidad completa: esta noche solo se analizó la carpeta del proyecto, que tardó 1 s. Un análisis grande usa más memoria mientras dura, porque guarda la lista de carpetas.
- El consumo con el navegador visible durante horas.

## Decisiones importantes

Todas están en `DECISIONES.md` con sus alternativas y motivos. Las principales:

- **Lista de procesos con `NtQuerySystemInformation`.** Con el método «de libro» (abrir cada proceso), 146 de 370 procesos no dejaban leer su CPU ni su memoria sin administrador.
- **Aplicación = procesos con el mismo nombre de ejecutable.** Por minuto se guardan las ~25 aplicaciones que más consumen y el resto se junta como «otras», para que la base no crezca sin control. Se conservan 30 días, con un tope de 200 MB.
- **Avisos de puertos nuevos solo para puertos accesibles desde otros equipos** (se puede cambiar). En la primera prueba real hubo 29 avisos de puertos locales de herramientas de desarrollo.
- **Notificaciones con una ventana propia**, desactivadas por omisión. Las notificaciones nativas hacen que Windows escriba en el registro (pregunta en `PREGUNTAS.md`).
- **El token viaja después de `#`** en la URL, que el navegador nunca envía al servidor, y luego en un encabezado propio.

## Problemas pendientes y cosas no verificadas

- **«Mostrar en el Explorador»** no se probó abriendo una ventana de verdad. Solo se probó que rechaza rutas que no están en un análisis guardado.
- **La tarjeta de batería** no se pudo ver: este equipo no tiene batería. La lectura sí se probó (devuelve «sin batería» sin errores).
- **La notificación propia** se probó creándola y cerrándola, pero no vi su aspecto en pantalla. Solo se muestra si activas la opción en Avisos.
- **El historial solo existe mientras Vigía está abierto.** Para tener datos de todo el día, ábrelo al iniciar sesión (instrucciones en `GUIA_DE_USO.md`). Si lo agregas al inicio, Vigía avisará una vez «Nuevo programa de inicio» por sí mismo.
- **Datos de esta noche**: la base `datos\vigia.db` ya tiene 58 minutos de historial. También tiene 29 avisos de puertos nuevos generados mientras probaba, antes de existir la opción de ignorar los puertos locales (puedes marcarlos como vistos), y dos análisis de la carpeta del proyecto. Las reglas de avisos tienen los valores por omisión.
- **Archivos que quedan bajo `target\tmp`**: los árboles de prueba del análisis de disco, con una unión de prueba que apunta a su propia carpeta. Están ahí porque la regla de la noche impedía borrar carpetas recursivamente. Se pueden borrar sin problema; `target\` está fuera de git.
- **La pantalla Ahora pide la muestra completa cada 2 s** (~200 KB, solo dentro del equipo). El consumo medido es bajo, pero se podría enviar solo lo que cambió.

## Siguientes pasos sugeridos

1. Responder las dos preguntas de `PREGUNTAS.md` (notificaciones nativas y avisos de puertos locales).
2. Probar en el uso diario «Mostrar en el Explorador» y un análisis de una unidad completa, y medir su memoria.
3. Si quieres, hacer que Vigía arranque con Windows (sección 3 de `GUIA_DE_USO.md`).
4. Posibles mejoras: el detalle de una aplicación agrupada (todos sus procesos juntos), y separar el disco y la red por proceso si algún día se usa con administrador.

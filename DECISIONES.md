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

## H2 · Historial

- **Aplicación = procesos con el mismo nombre de ejecutable** (sin distinguir mayúsculas) · agrupar por ruta · más simple y es como lo piensa una persona ("Chrome"); la ruta y la descripción se guardan aparte en la tabla `apps`.
- **Por minuto se guardan solo las 12 apps con más CPU, las 12 con más memoria y las 5 con más E/S, más una fila `(otros)`** con la suma del resto · guardar todas · con ~370 procesos serían ~100 filas por minuto y ~4 millones de filas al mes; así son ~15–25 filas por minuto y los totales siguen cuadrando.
- **Promedios del minuto cuentan como 0 las muestras en que la app no estaba** · promediar solo cuando estaba · así la suma de las apps da el total del sistema.
- **En consultas por rango, `cpu_prom` y `mem_prom` se promedian sobre todo el rango**; además se entrega `mem_prom_activa` (solo mientras estuvo abierta) · un solo promedio · ordenar por el promedio del rango premia lo que más consumió en total.
- **Retención: 30 días, y tope de 200 MB** (borrando por horas desde lo más viejo, nunca la última hora; los análisis de disco tienen su propio tope de ¼) · borrar por días · por horas se pasa menos del tope. `auto_vacuum=INCREMENTAL` + `PRAGMA incremental_vacuum` (recorriendo todas sus filas) devuelve el espacio sin reescribir el archivo.
- **WAL** (`journal_mode=WAL`) · modo por defecto · la interfaz puede leer mientras el colector escribe.
- **Hora como parámetro en todo el historial** · un trait `Reloj` · las pruebas simulan horas de datos con solo pasar números (reloj simulado), sin trait extra.
- **Pruebas que escriben en disco usan `CARGO_TARGET_TMPDIR`** (dentro de `target/`) y borran solo sus archivos concretos · carpeta temporal del sistema · la regla es escribir solo dentro de la carpeta del proyecto.

## H3 · Interfaz y servidor

- **Token en la URL después de `#`** (`/#token=...`), guardado en `sessionStorage` y enviado en el encabezado `X-Vigia-Token` · cookie · el fragmento `#` nunca viaja al servidor ni queda en registros, y un encabezado propio no lo envía el navegador solo (protege contra CSRF). La página lo quita de la barra de direcciones al cargarse.
- **La página `/` se sirve sin token** (solo con `Host` válido) · exigir token también · la página no contiene datos; todos los datos van por `/api/...`, que sí lo exige.
- **`Content-Security-Policy: default-src 'none'`** con scripts y estilos en línea · archivos JS/CSS separados · el requisito es una sola página; la política impide cargar cualquier recurso externo.
- **Puerto fijo 47613, con respaldo a uno libre** · siempre aleatorio · un puerto estable facilita marcadores; si está ocupado se usa otro.
- **Una sola instancia**: `datos/vigia_estado.json` guarda puerto, token y PID; un segundo arranque abre la página de la instancia existente · permitir varias · evita dos colectores escribiendo la misma base.
- **`detener_vigia.bat` → `vigia --detener`**, que llama a `POST /api/apagar` con el token · `taskkill` · se cierra solo la instancia propia y de forma ordenada (guarda el último minuto). Cerrar la ventana de consola también guarda (vía `SetConsoleCtrlHandler`).
- **Ventana de consola minimizada** (`start /min`) · ejecutable sin consola (`windows_subsystem`) · más simple y deja ver mensajes de error; cerrarla cierra Vigía.
- **Opción `--datos CARPETA`** · solo carpeta fija · permite probar el arranque real sin tocar los datos del usuario.
- **Gráficos dibujados a mano en `<canvas>`** · biblioteca de gráficos · sin dependencias ni CDN, como pide el proyecto.
- **Modo oscuro**: sigue al sistema la primera vez y luego recuerda la elección en `localStorage`.
- **Tabla de procesos completa cada 2 s** (sin paginación) · paginar · ~370 filas se pintan en milisegundos; la actualización se pausa si la pestaña no está visible.

## H4 · Disco

- **Listado con `FindFirstFileExW` + `FindNextFileW`** (`FindExInfoBasic`, `FIND_FIRST_EX_LARGE_FETCH`) · `std::fs::read_dir` + `metadata` · entrega nombre, tamaño y atributos en una pasada, sin abrir cada archivo, y muestra el atributo de punto de reanálisis.
- **Se saltan las carpetas con atributo `REPARSE_POINT`** (enlaces simbólicos, uniones) y se cuentan como «enlaces omitidos» · seguirlas · evita contar dos veces y los ciclos infinitos (probado con una unión que apunta a su propia raíz).
- **Archivos solo en la nube (OneDrive) se suman aparte** (atributos `OFFLINE` / `RECALL_ON_DATA_ACCESS`) · sumarlos al total · no ocupan espacio en el disco.
- **Tamaño = tamaño lógico del archivo** · tamaño en disco (clústeres, compresión) · obtener el tamaño real asignado requiere abrir cada archivo (`GetCompressedFileSizeW`), mucho más lento.
- **Recorrido iterativo con pila propia** · recursión · una ruta muy profunda no desborda la pila del hilo.
- **Prefijo `\\?\`** en las rutas · nada · permite rutas de más de 260 caracteres.
- **Se guarda un árbol resumido** (carpetas ≥ 0,1 % del total, máx. 4000 nodos y 12 niveles), los 100 archivos más grandes y 60 tipos · guardar todo · una unidad completa puede tener cientos de miles de carpetas; el resumen cabe en unos cientos de KB de JSON.
- **Se conservan los últimos 10 análisis por raíz; la comparación usa el análisis completo anterior** · comparar también con cancelados · un análisis cancelado está incompleto y daría diferencias falsas (igual se guarda y se puede ver, marcado como incompleto).
- **«Mostrar en el Explorador» solo acepta rutas que aparecen en un análisis guardado** · aceptar cualquier ruta · la API no debe servir para abrir rutas arbitrarias. Usa `explorer.exe /select,"ruta"`. No se probó abrir de verdad (solo el rechazo de rutas ajenas) para no abrir ventanas sin supervisión.
- **Pruebas solo sobre la carpeta del proyecto y árboles generados en `target/tmp`**, como pide el plan. Los árboles y la unión de prueba se dejan ahí (no se borran carpetas recursivamente).

## H5 · Inicio y conexiones

- **Claves revisadas**: `Run` y `RunOnce` de HKCU y HKLM, y `Run` de `WOW6432Node`, más las carpetas de inicio del usuario y comunes · también servicios y tareas programadas · el plan pide claves Run y carpetas de inicio; servicios y tareas serían otro tema.
- **Estado «desactivada»** leído de `Explorer\StartupApproved` (primer byte impar = desactivada) · no mostrarlo · así coincide con el Administrador de tareas. Es solo lectura.
- **Los accesos directos `.lnk` se muestran por su nombre**, sin resolver su destino · resolverlos con `IShellLink` (COM) · agrega COM solo para esto.
- **Revisión cada 60 s en un hilo propio, con la referencia guardada en la base** · comparar solo en memoria · así también se detectan cambios hechos mientras Vigía estaba cerrado. La primera revisión solo guarda la referencia.
- **Puertos conocidos se acumulan** y la clave es protocolo + puerto + alcance + nombre del programa (no el PID) · comparar solo contra la revisión anterior · un programa que reinicia o un puerto que se cierra y reabre no vuelve a avisar.
- **Solo TCP genera avisos de puerto nuevo** · también UDP · los puertos UDP cambian a cada rato (consultas DNS) y serían ruido. UDP sí se muestra en la lista.
- **Sin resolución inversa de DNS** · mostrar nombres de dominio · enviaría consultas a internet por cada dirección; la regla es que nada salga del PC salvo cargo.
- **«Accesible desde»**: 0.0.0.0/:: = toda la red; IP de una interfaz = la red de esa dirección; 127.x/::1 = solo este equipo.

## Revisión de H3 (ver `REVISION_H3.md`)

- **Instancia única con mutex con nombre** (`Local\Vigia-<hash de la carpeta de datos>`) · archivo de bloqueo · el mutex es atómico y Windows lo libera solo si el proceso muere.
- **Un hilo por petición HTTP (máx. 16)** · un solo hilo · un cliente lento no bloquea la interfaz ni el cierre.
- **Minuto repetido = combinar ponderando por muestras** · reemplazar · reiniciar Vigía dentro de un minuto no pierde datos.

## H6 · Avisos

- **Reglas guardadas como JSON en la tabla `config`**, con valores por omisión (memoria > 90 % durante 5 min, un proceso > 50 % de CPU durante 5 min, unidad con < 10 % libre, inicio y puertos activados) · archivo aparte · una sola base de datos que respaldar.
- **El motor usa la hora de cada muestra, no el reloj** · leer el reloj · las pruebas simulan minutos con muestras falsas.
- **«Sostenido» = todas las muestras seguidas por encima del umbral**; una sola muestra por debajo reinicia la cuenta · promedio móvil · es la definición más simple y fácil de explicar.
- **CPU por proceso en % del equipo completo**, igual que la tabla · % de un núcleo · coherente con lo que se ve en pantalla.
- **No repetir el mismo aviso antes de 1 hora (6 horas para disco)**, por regla y por sujeto (proceso, unidad) · avisar en cada muestra · evita una avalancha de avisos.
- **Los valores que llegan de la interfaz se corrigen a rangos razonables** (`Reglas::corregidas`) · rechazar · más amable y igual de seguro.
- **Puertos nuevos: por omisión solo avisan los accesibles desde otros equipos** (`puerto_solo_red`) · avisar de todos · en la primera prueba real hubo 29 avisos, casi todos de herramientas de desarrollo en 127.0.0.1. Se puede desactivar en la pantalla Avisos.
- **La referencia de inicio y puertos se actualiza aunque la regla esté desactivada** · no revisar · si se vuelve a activar no aparece una avalancha de cambios viejos.

## Después de los hitos (extras)

- **Resumen semanal** como pantalla propia («Semana»): CPU y memoria promedio, 10 aplicaciones que más CPU y memoria usaron, 5 momentos de mayor carga (separados al menos 1 hora, para no mostrar 5 minutos seguidos de la misma tarde) con la aplicación que más consumía, y barras por día · solo una tabla · la separación hace que los picos digan algo distinto cada uno. Las barras por día se agrupan en el navegador, que conoce la zona horaria local.
- **CSV pensado para Excel en español**: separador `;`, coma decimal, UTF-8 con BOM, fechas en hora local con `SystemTimeToTzSpecificLocalTime` (respeta el horario de verano de cada fecha) · separador `,` · Excel en español no abre bien los CSV con coma. La descarga se pide con `fetch` para poder enviar el token.
- **Notificaciones: ventana propia** (Win32 + GDI, esquina inferior derecha, no roba el foco, se cierra a los 8 s o con un clic, máximo 3 apiladas), **desactivada por omisión** · globo de `Shell_NotifyIconW` o *toast* · ambas hacen que Windows escriba en el registro (ver `PREGUNTAS.md`). La ventana corre en su propio hilo con su bucle de mensajes.
- **Batería** con `GetSystemPowerStatus`, leída cada 30 s junto con las unidades; la tarjeta solo aparece si el equipo tiene batería · ocultar siempre · en este equipo de escritorio no se ve, pero el código queda probado contra la lectura real.

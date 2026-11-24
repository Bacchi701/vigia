# Vigía: trabajo nocturno sin supervisión

Hoy es martes 24 de noviembre de 2026. Voy a dormir y quiero que trabajes solo, sin supervisión, hasta terminar todo el plan. Tu tarea es construir de principio a fin «Vigía», un monitor de mi propio PC, escrito en Rust, dentro de esta carpeta.

**Por qué lo quiero:** el Administrador de Tareas de Windows solo muestra lo que pasa ahora. Yo quiero saber qué pasó antes (qué programa tenía lento el PC ayer), qué me llena el disco, qué se agregó al inicio de Windows y qué programas tienen conexiones abiertas, todo en un solo lugar y con avisos. Además estoy aprendiendo Rust y me interesa el bajo nivel: voy a estudiar tu código para aprender cómo se le pide esta información a Windows, así que la claridad del código importa tanto como que funcione.

**Este proyecto es tuyo:** tú decides cómo se construye y vas a seguir construyendo sobre él.

## Qué es Vigía

Una app local para Windows 11 que **solo observa**: lee información del sistema y nunca lo modifica.
- Un solo ejecutable en Rust (`vigia.exe`) que incluye el colector, el historial y un servidor local.
- Los datos se obtienen llamando directamente a la API de Windows con el crate `windows`. No uses crates que ya lo resuelven todo, como `sysinfo`: quiero ver las llamadas reales.
- Los datos se guardan en SQLite dentro de esta carpeta y nunca salen de mi PC.
- El servidor escucha únicamente en `127.0.0.1`. La interfaz es una sola página HTML, CSS y JavaScript, sin dependencias ni CDN, en español, incluida dentro del ejecutable.
- Como muestra información sensible (procesos, rutas, conexiones), el servidor rechaza las peticiones cuyo `Host` u `Origin` no sea el suyo y exige un token generado al iniciar.
- Se abre con doble clic en `iniciar_vigia.bat`, que ejecuta la versión compilada en modo release y abre el navegador. `detener_vigia.bat` la cierra.
- Debe ser liviano: apunta a menos de 1% de CPU y menos de 50 MB de memoria. Mídelo y repórtalo.

## Cuándo terminar

- Hoy no hay hora límite. Trabaja hasta completar todos los hitos y después la lista «Después de los hitos», en orden.
- Cuando todo esté hecho y verificado, escribe `INFORME_FINAL.md`, haz el commit final y detente. No inventes trabajo fuera del plan.
- Anota la hora de inicio como `INICIO` en `PROGRESO.md` (`powershell -NoProfile -Command "Get-Date -Format o"`) y la hora al cerrar cada hito.
- Si la sesión se corta por el límite de uso y se reanuda, continúa desde `PROGRESO.md`.
- Cada hito termina con commit y `PROGRESO.md` al día. Si al iniciar encuentras `PROGRESO.md`, continúa desde ahí.

## Cómo terminan tus turnos (importante)

Estaré durmiendo. Un mensaje tuyo sin llamada a herramienta termina el turno, y el trabajo queda detenido hasta que yo despierte. No quiero ninguno de estos finales mientras quede trabajo del plan:
1. Un resumen de lo hecho que anuncia el siguiente paso, pero no lo empieza.
2. Una oferta de continuar «si te parece bien».
3. Una lista de decisiones para mí cuando ninguna bloquea el resto del trabajo.
4. Detenerte a informar porque el turno ya fue largo o porque terminaste un hito.

Las notas de avance son bienvenidas, pero escríbelas en `PROGRESO.md` o en el mismo mensaje que tu siguiente llamada a herramienta, y sigue. Si te ves ofreciendo esperar, bórralo y haz lo siguiente. Solo debes detenerte cuando todo el plan esté terminado, o si nada puede avanzar sin mí.

## Entorno (ya verificado en mi PC)

- Windows 11 x64 con Rust 1.100 (toolchain MSVC, `cargo` y `rustc` en el PATH) y las herramientas de compilación de C++ de Visual Studio. Un proyecto nuevo compila y enlaza bien. También hay git 2.55.
- No uses WSL en este proyecto: la información debe venir del Windows real.
- Internet solo para que `cargo` descargue crates de crates.io. Nada más.
- **En H0, agrega y compila de una vez todas las dependencias que planeas usar.** Así, si alguna descarga queda bloqueada, se nota al principio, mientras todavía estoy mirando.
- Usa pocas dependencias y conocidas. Sugerencia: `windows` para la API de Windows, `rusqlite` con la característica `bundled` para SQLite, `serde` y `serde_json`, y un servidor HTTP pequeño y síncrono como `tiny_http`. Anota en `DECISIONES.md` cada crate y para qué lo usas. Guarda `Cargo.lock` en git.
- La primera compilación de las dependencias puede tardar varios minutos: dale un límite de tiempo amplio.

## Qué puedes hacer sin preguntar

- Crear, editar y mover archivos dentro de esta carpeta, y borrar archivos concretos.
- Ejecutar `cargo build`, `cargo test`, `cargo run`, `cargo fmt`, `cargo clippy` y `cargo add`.
- **Leer** información del sistema: procesos, memoria, uso de CPU, discos, adaptadores de red, tabla de conexiones, claves de inicio del registro y carpetas de inicio.
- Crear procesos de prueba propios (por ejemplo, el mismo ejecutable en un modo que consuma CPU unos segundos) para comprobar que Vigía los detecta. Ciérralos siempre al terminar.
- Levantar el colector y el servidor dentro de las pruebas, cerrándolos al terminar.
- Reintentar un comando que falló por un error común después de diagnosticarlo.
- Hacer commits locales con git.

## Límites (pocos, y con su motivo)

- Vigía solo observa. No termina, suspende ni modifica procesos ajenos; no escribe en el registro; no cambia programas de inicio, servicios, tareas programadas ni reglas de firewall. Tampoco lo hagas tú durante el trabajo. Motivo: nadie supervisa esta noche y un error ahí puede dañar mi sistema.
- No registres Vigía para que arranque con Windows. Deja las instrucciones en la guía y lo haré yo.
- Sin privilegios de administrador. Si un proceso del sistema no deja leer algún dato, muestra lo que haya y sigue: nunca debe fallar por eso.
- Escribe solo dentro de esta carpeta. Durante la noche, los análisis de disco se prueban únicamente sobre esta carpeta y sobre un árbol de prueba que tú generes.
- Sin captura de paquetes ni lectura del contenido de las conexiones: solo qué proceso tiene qué conexión.
- No instales nada fuera de los crates del proyecto: ni herramientas con `cargo install`, ni componentes con `rustup`, ni otros programas.
- Sin `git push`, sin borrados recursivos de carpetas (tampoco `cargo clean`), sin comandos destructivos de git (`reset --hard`, `clean -fd`) y sin dejar procesos abiertos en segundo plano.
- Si el sistema de permisos bloquea una acción o un filtro de seguridad rechaza una parte, es un bloqueo deliberado: no lo reintentes ni lo rodees. Varios bloqueos seguidos detienen la sesión a esperar mi permiso. Anótalo en `PROGRESO.md` y sigue con otra tarea. Si lo bloqueado es la descarga de un crate, reemplázalo por código propio (por ejemplo, declarando a mano las funciones de Windows con `extern "system"`).

## Cómo escribir el código

- Concentra todo el `unsafe` en un módulo de acceso a Windows, detrás de funciones seguras. El resto del programa no usa `unsafe`.
- En cada llamada a la API de Windows, comenta qué función es, qué devuelve y por qué elegiste esa. Voy a estudiarlo.
- Errores con `Result`, sin `unwrap()` fuera de las pruebas.
- Código formateado con `cargo fmt` y sin avisos de `cargo clippy`.

## Cómo trabajar

- No hagas preguntas ni esperes respuestas. Elige la opción razonable más simple y anótala en `DECISIONES.md` (decisión, alternativas y motivo). Lo que necesite mi opinión va a `PREGUNTAS.md`.
- Ante errores comunes, diagnostica y resuelve con lo que tienes. Solo los bloqueos deliberados se dejan de lado.
- Ningún hito está terminado sin pruebas que hayas ejecutado tú en esta sesión. Antes de escribir un avance en `PROGRESO.md`, compruébalo contra un resultado de herramienta de esta sesión.
- Mantén en `PROGRESO.md` la lista de hitos con su estado, y úsala para decidir qué sigue.
- No uses subagentes, salvo uno: al terminar H3, un subagente revisor que no haya visto el código revisa el colector, el historial y la interfaz. Corrige lo que encuentre o explica por qué no.
- Desde H3, mantén la app usable en cada commit: `iniciar_vigia.bat` siempre debe funcionar.
- Todo comando que pueda colgarse corre con límite de tiempo.

## Hitos (cada uno termina con pruebas pasando y un commit)

**H0 · Preparación.** `cargo init`, dependencias agregadas y compiladas, una llamada mínima a la API de Windows funcionando, `.gitignore` (`target/`, base de datos, temporales), `PROGRESO.md`, `DECISIONES.md` y un `ARQUITECTURA.md` breve.

**H1 · Colector.** Totales del sistema: uso de CPU, memoria, espacio libre por unidad y bytes de red por adaptador. Por proceso: nombre, PID, ruta del ejecutable, uso de CPU, memoria, bytes de entrada y salida, hora de inicio, y la descripción y la empresa que declara el ejecutable. Pruebas: el propio proceso aparece con valores coherentes, y un proceso de prueba que consume CPU o memoria se detecta.

**H2 · Historial.** Muestras en vivo cada pocos segundos en memoria, y resúmenes por minuto guardados en SQLite, agrupados por aplicación. Retención de 30 días y un tope de tamaño de la base de datos. Consultas: qué aplicaciones usaron más CPU y memoria en un rango, y qué estaba pasando en un momento dado. Pruebas con reloj simulado.

**H3 · Interfaz.** «Ahora»: tabla en vivo que se puede ordenar y filtrar, con gráficos de los últimos minutos. «Historial»: línea de tiempo donde elijo un momento y veo qué consumía más entonces. «Aplicaciones»: uso por aplicación en el día y la semana. Panel de detalle «¿Qué es esto?» con ruta, empresa, descripción y hora de inicio. Modo oscuro. En el diseño evita: fondo crema o blanco roto, palabras en cursiva como acento en los títulos, secciones numeradas «01/02/03», etiquetas en fuente monoespaciada y botones en forma de píldora. Pruebas de la API y de que la página carga. Al terminar, la revisión del subagente.

**H4 · Disco.** Análisis bajo demanda de la carpeta o unidad que yo elija: tamaño por carpeta en árbol, archivos más grandes y espacio por tipo de archivo. Salta enlaces y puntos de reanálisis, tolera errores de permiso, se puede cancelar y guarda el resultado para comparar con el análisis anterior y ver qué creció. Nunca borra nada: solo ofrece abrir la ubicación en el Explorador.

**H5 · Inicio y conexiones.** Programas de inicio leídos de las claves `Run` del registro y de las carpetas de inicio, con aviso cuando aparece o desaparece una entrada entre dos revisiones. Conexiones: qué procesos tienen conexiones abiertas y hacia qué direcciones, y qué puertos están en escucha, con aviso cuando aparece un puerto en escucha nuevo.

**H6 · Avisos.** Reglas configurables, mostradas dentro de la app: memoria del sistema alta durante varios minutos, un proceso con CPU alta sostenida, poco espacio libre en una unidad, entrada de inicio nueva y puerto en escucha nuevo. Pruebas con datos simulados.

**H7 · Entrega.** Compilación en modo release; `iniciar_vigia.bat` y `detener_vigia.bat`; README; `GUIA_DE_USO.md` (cómo leer cada pantalla, cómo hacer que arranque con Windows si yo quiero, y un mapa del código para estudiarlo, con el orden en que conviene leerlo); pruebas completas; la medición del consumo del propio Vigía; e `INFORME_FINAL.md`.

## Después de los hitos (en este orden)

1. Resumen semanal: las aplicaciones que más consumieron y los momentos de mayor carga.
2. Exportar datos a CSV.
3. Notificaciones de Windows para los avisos.
4. Estado de la batería, si el equipo tiene.

## Formato de PROGRESO.md

`INICIO`, lista de hitos con su estado, resultado de las pruebas de cada hito terminado, qué sigue, problemas abiertos y una línea «Cómo reanudar».

## Formato de INFORME_FINAL.md

Escríbelo para alguien que no vio nada de tu trabajo. En las primeras frases: si la app funciona y cómo abrirla. Después: qué se construyó, qué datos no se pueden obtener sin administrador, resultados de las pruebas, consumo medido de Vigía, decisiones importantes, problemas pendientes y siguientes pasos. Frases claras, sin términos que hayas inventado durante el trabajo y solo con afirmaciones verificadas.

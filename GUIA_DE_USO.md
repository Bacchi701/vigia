# Guía de uso de Vigía

## 1. Abrir y cerrar

- **Abrir**: doble clic en `iniciar_vigia.bat`.
  - La primera vez compila la versión release (`cargo build --release`, unos minutos).
  - Después inicia `target\release\vigia.exe` en una ventana minimizada llamada «Vigia» y abre el navegador.
  - Si Vigía ya estaba abierto, solo abre otra pestaña con la página: nunca hay dos Vigías a la vez.
- **Cerrar**: doble clic en `detener_vigia.bat`. Vigía guarda el último minuto y se cierra. Cerrar la ventana «Vigia» también lo cierra (y también guarda).
- **Si cambias el código**: compila con `cargo build --release` con Vigía cerrado (Windows no deja reemplazar un `.exe` que está en uso).
- **La página dice «Falta la llave de acceso»**: por seguridad, cada arranque genera un token nuevo que solo conoce la pestaña que abrió Vigía. Abre Vigía otra vez con `iniciar_vigia.bat` y se abrirá una pestaña válida.
- **Franja roja «Sin conexión con Vigía»**: Vigía está cerrado. Ábrelo de nuevo.

Opciones de la línea de comandos (`target\release\vigia.exe --ayuda`): `--sin-navegador`, `--puerto N`, `--datos CARPETA`, `--detener`, `--muestra` (imprime una muestra en la consola).

## 2. Cómo leer cada pantalla

### Ahora
- **Tarjetas**: CPU del equipo, memoria en uso, red (bajada / subida) y lectura+escritura de todos los procesos, con los últimos 10 minutos en el gráfico.
- **Unidades**: espacio libre de cada unidad fija o extraíble (en rojo si está ocupada en más de un 90 %).
- **Tabla de procesos**: se actualiza cada 2 s. Haz clic en un encabezado para ordenar; escribe en el filtro para buscar por nombre, empresa, ruta o PID. «Agrupar por aplicación» suma todos los procesos con el mismo ejecutable (los 20 `chrome.exe` como uno).
  - **CPU**: porcentaje del equipo completo (0–100 %), como el Administrador de tareas. Un programa que ocupa un núcleo entero en un equipo de 6 núcleos lógicos marca ~17 %.
  - **Memoria**: memoria física privada del proceso (la columna «Memoria» del Administrador de tareas).
  - **E/S**: bytes por segundo que el proceso lee y escribe (disco, red y dispositivos juntos; Windows no los separa por proceso sin permisos de administrador).
- **Clic en una fila → «¿Qué es esto?»**: ruta del ejecutable, empresa y descripción que declara el archivo, quién lo inició, hora de inicio, las tres medidas de memoria y gráficos de sus últimos minutos. La empresa y la descripción las escribe quien compiló el programa: son lo que el programa *dice* ser, no una garantía.

### Historial
- Elige un rango (1 h a 30 días). La línea azul es la CPU promedio, la banda azul clara la CPU máxima de cada minuto y la línea morada la memoria.
- **Haz clic en el gráfico** (o elige fecha y hora arriba a la derecha) para ver qué aplicaciones consumían más CPU y memoria en ese momento.
- Los huecos son momentos en que Vigía estaba cerrado.

### Aplicaciones
- Uso por aplicación hoy, ayer, en 7 o en 30 días, ordenado por CPU, memoria o lectura/escritura.
- «Promedio» cuenta todo el período, incluso cuando la aplicación estaba cerrada: una aplicación que usó 50 % durante una hora de un día de 24 h aparece con ~2 %.
- «Minutos destacada»: minutos en que estuvo entre las que más consumían. Vigía guarda por minuto solo las ~25 aplicaciones más grandes y junta el resto como «otras».

### Disco
- Escribe una carpeta o unidad (o usa los botones `C:\`, `D:\`...) y presiona **Analizar**. Se puede cancelar.
- Muestra el **árbol de carpetas** (clic para abrir cada una), el **espacio por tipo de archivo** y los **100 archivos más grandes**.
- Desde el segundo análisis de la misma carpeta aparece **«Qué creció»**, comparando con el análisis anterior.
- **«Mostrar en el Explorador» / «Abrir»** abre la carpeta en el Explorador con el archivo marcado. **Vigía nunca borra nada**; si quieres liberar espacio, hazlo tú desde el Explorador.
- Se saltan enlaces y uniones (para no contar dos veces), y las carpetas sin permiso se cuentan como «sin permiso». Los archivos de OneDrive que están solo en la nube se informan aparte porque no ocupan este disco.

### Inicio
- Programas que Windows abre al iniciar sesión: claves `Run` y `RunOnce` del registro (usuario y equipo) y carpetas de inicio.
- «Desactivada» significa que se desactivó desde el Administrador de tareas. «No encontrado»: la entrada apunta a un archivo que ya no existe.
- Vigía revisa cada minuto y avisa si aparece o desaparece una entrada. Para desactivar una, usa el Administrador de tareas → Aplicaciones de arranque: Vigía no cambia nada.

### Conexiones
- **Puertos en escucha**: programas esperando conexiones. «Toda la red» o «la red de esa dirección» (en naranja) significa que otros equipos podrían conectarse si el firewall lo permite; «Solo este equipo» es 127.0.0.1.
- **Conexiones activas**: conexiones TCP con su dirección remota y si es local, red local o internet. Vigía no lee el contenido ni averigua nombres de dominio.

### Avisos
- **Reglas** (se guardan al presionar «Guardar reglas»): memoria alta sostenida, un proceso con CPU alta sostenida, poco espacio libre, cambios en el inicio, puertos nuevos (por omisión solo los accesibles desde otros equipos).
- Un mismo aviso no se repite antes de una hora (seis horas para el disco). El menú muestra cuántos avisos no has visto.

## 3. Hacer que Vigía arranque con Windows (si quieres)

Vigía no se registra solo. Si quieres que arranque al iniciar sesión, la forma más simple:

1. Presiona `Win + R`, escribe `shell:startup` y presiona Enter. Se abre tu carpeta de inicio.
2. Con el botón derecho del ratón sobre `iniciar_vigia.bat` → «Mostrar más opciones» → «Crear acceso directo».
3. Mueve ese acceso directo a la carpeta de inicio.

Para quitarlo, borra el acceso directo de esa carpeta. Ten en cuenta:
- La primera vez después de hacerlo, Vigía mostrará el aviso «Nuevo programa de inicio»: es él mismo.
- Si prefieres que no se abra el navegador al iniciar sesión, edita las propiedades del acceso directo y cambia el destino a `"C:\ruta\a\vigia\target\release\vigia.exe" --sin-navegador`, con «Ejecutar: Minimizada».

## 4. Mapa del código (para estudiarlo)

Orden de lectura sugerido, de lo más concreto (cómo se le pide la información a Windows) a lo más general:

1. **`src/win/mod.rs`**: único módulo con `unsafe`. Ayudas comunes: manijas que se cierran solas (`Manija`), conversión de texto a UTF-16 y de `FILETIME` a hora Unix.
2. **`src/win/sistema.rs`**: memoria (`GlobalMemoryStatusEx`), CPU total (`GetSystemTimes`), unidades (`GetLogicalDriveStringsW`, `GetDriveTypeW`, `GetDiskFreeSpaceExW`).
3. **`src/win/proceso.rs`**: lista de procesos con `NtQuerySystemInformation` (cómo lo hace el Administrador de tareas), lectura de campos "reservados" y ruta del ejecutable con `OpenProcess` + `QueryFullProcessImageNameW`, o sin abrir el proceso (clase 88 + `QueryDosDeviceW`).
4. **`src/win/version.rs`**: descripción y empresa de un ejecutable (`GetFileVersionInfoW`, `VerQueryValueW`).
5. **`src/win/red.rs`**: bytes por adaptador (`GetIfTable2`) y conexiones por proceso (`GetExtendedTcpTable`, `GetExtendedUdpTable`).
6. **`src/win/archivos.rs`**: listado de carpetas con `FindFirstFileExW` (atributos, puntos de reanálisis).
7. **`src/win/registro.rs`**: lectura del registro (`RegOpenKeyExW` con `KEY_READ`, `RegEnumValueW`) y carpetas conocidas (`SHGetKnownFolderPath`).
8. **`src/win/sistema_extra.rs`**: token aleatorio (`BCryptGenRandom`), abrir navegador/Explorador (`ShellExecuteW`), cierre de consola (`SetConsoleCtrlHandler`), instancia única (`CreateMutexW`).
9. **`src/colector.rs`**: de contadores acumulados a tasas (CPU %, bytes/s). Es el corazón conceptual: dos lecturas y una resta.
10. **`src/historial.rs`** y **`src/base.rs`**: muestras en vivo, resumen por minuto por aplicación, SQLite, retención y consultas. Las pruebas usan un reloj simulado.
11. **`src/disco.rs`**, **`src/inicio.rs`**, **`src/conexiones.rs`**, **`src/vigilante.rs`**, **`src/avisos.rs`**: la lógica de cada pantalla. No usan `unsafe`.
12. **`src/app.rs`**: arranca los hilos (colector, servidor, vigilante) y comparte el estado con `Arc<Mutex<…>>`.
13. **`src/servidor.rs`**: API JSON, comprobación de `Host`, `Origin` y token.
14. **`src/main.rs`**: línea de comandos, instancia única, `--detener`.
15. **`ui/index.html`**: la interfaz (HTML, CSS y JavaScript sin dependencias). Los gráficos se dibujan a mano en `<canvas>`.
16. **`tests/`**: pruebas de integración contra el Windows real (procesos de prueba que consumen CPU/memoria, árbol de disco con una unión en bucle, servidor levantado dentro de la prueba).

Reglas que el compilador hace cumplir: `#![deny(unsafe_code)]` en todo el programa salvo `src/win/`, y `unwrap()`/`expect()` prohibidos fuera de las pruebas.

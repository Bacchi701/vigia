# Vigía

Monitor local para Windows 11, escrito en Rust. **Solo observa**: lee información del sistema y nunca lo modifica. Todo queda en esta carpeta; nada sale del PC.

Qué muestra, en una sola página web local:

- **Ahora**: procesos con CPU, memoria y lectura/escritura en vivo, gráficos de los últimos 10 minutos, unidades y red.
- **Historial**: qué consumía más en cualquier momento de los últimos 30 días.
- **Aplicaciones**: uso por aplicación hoy, ayer, en 7 y en 30 días.
- **Disco**: qué carpetas, archivos y tipos de archivo ocupan más espacio, y qué creció desde el análisis anterior.
- **Inicio**: programas que arrancan con Windows, con aviso cuando aparece o desaparece uno.
- **Conexiones**: qué programas tienen conexiones abiertas, a qué direcciones y qué puertos están en escucha.
- **Avisos**: reglas configurables (memoria alta, CPU alta sostenida, poco disco, inicio nuevo, puerto nuevo).

## Uso

- **Abrir**: doble clic en `iniciar_vigia.bat`. La primera vez compila (unos minutos); luego abre el navegador. Queda una ventana minimizada llamada «Vigia»: si la cierras, Vigía se cierra.
- **Cerrar**: doble clic en `detener_vigia.bat`.

Requisitos para compilar: Rust (toolchain MSVC) y las herramientas de compilación de C++ de Visual Studio. No necesita permisos de administrador.

Más detalle, incluido cómo hacer que arranque con Windows y un mapa del código para estudiarlo: [`GUIA_DE_USO.md`](GUIA_DE_USO.md).

## Desarrollo

```powershell
cargo test
```

```powershell
cargo clippy --all-targets -- -D warnings
```

```powershell
cargo build --release
```

Otros documentos: [`ARQUITECTURA.md`](ARQUITECTURA.md) (diseño), [`DECISIONES.md`](DECISIONES.md) (por qué se hizo cada cosa así), [`PROGRESO.md`](PROGRESO.md) (bitácora de construcción), [`INFORME_FINAL.md`](INFORME_FINAL.md).

## Privacidad y seguridad

- Los datos se guardan en `datos\vigia.db` (SQLite) dentro de esta carpeta.
- El servidor escucha solo en `127.0.0.1` y rechaza peticiones con `Host` u `Origin` ajenos; la API exige un token aleatorio que se genera en cada arranque.
- No hay captura de paquetes ni lectura del contenido de las conexiones, y no se consultan nombres de dominio.

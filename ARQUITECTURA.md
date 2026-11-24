# Arquitectura de Vigía

Un solo ejecutable (`vigia.exe`) con varios hilos que comparten estado detrás de `Mutex`:

```
            +-------------------+       +-------------------+
 Windows -->| win (único unsafe)|<------| colector          |  cada 2 s: procesos, CPU,
  API       +-------------------+       +-------------------+  memoria, discos, red
                  ^      ^                     |
                  |      |                     v
                  |      |              +-------------------+   SQLite (datos/vigia.db)
                  |      |              | historial         |-> resúmenes por minuto,
                  |      |              +-------------------+   retención y tope de tamaño
                  |      |                     |
     disco ------+      +---- inicio,          v
  (análisis bajo         conexiones     +-------------------+
   demanda)                             | avisos (reglas)   |
                                        +-------------------+
                                               |
                                        +-------------------+    solo 127.0.0.1,
                                        | servidor HTTP     |--> comprueba Host/Origin
                                        +-------------------+    y token
                                               |
                                        ui/index.html (incluida en el .exe)
```

- `src/win/`: el único lugar con `unsafe`. Cada función pública es segura y devuelve `Result`.
- `src/colector.rs`: convierte contadores acumulados de Windows en tasas (CPU %, bytes/s).
- `src/historial.rs`: muestras en vivo en memoria y resúmenes por minuto en SQLite.
- `src/disco.rs`, `src/inicio.rs`, `src/conexiones.rs`, `src/avisos.rs`: funciones de cada pantalla.
- `src/servidor.rs`: API JSON y página.
- `src/app.rs`: arranca y detiene todos los hilos.
- `src/main.rs`: argumentos de línea de comandos.

Las funciones de lógica reciben la hora como parámetro (`ahora_ms`), así las pruebas usan un reloj simulado.

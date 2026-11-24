# Revisión de H3 (subagente revisor, sin ver el código antes)

Hallazgos y qué se hizo con cada uno. Estado: ✔ corregido · ✗ no se corrige (motivo) · … pendiente.

## Errores reales (colector, historial, servidor)
- A1 … `PRAGMA incremental_vacuum` con `execute_batch` solo libera una página por llamada: el archivo nunca se encoge. Recorrer todas las filas.
- A2 … El tope de tamaño mide toda la base (incluidos análisis de disco) pero solo borra historial: podría vaciar el historial. Medir/limitar aparte, cortar si borrar no ayuda, podar `apps` huérfanas.
- A3 … `INSERT OR REPLACE` de un minuto incompleto pisa el anterior (reinicio en el mismo minuto, reloj que retrocede).
- A4 … Si una muestra falla, `instante_anterior` ya se actualizó y la siguiente tasa sale al doble.
- A5 … El servidor lee el cuerpo antes de validar: un cliente local lento bloquea el único hilo y el cierre. `--detener` dice «se cerró» aunque no se confirme.
- A6 … Dos arranques muy seguidos pueden crear dos instancias (falta un bloqueo atómico).
- A7 … Desbordamiento de i64 con parámetros extremos en la URL.

## Interfaz
- B1 … «Agrupar por aplicación» + clic abre «Este proceso ya terminó».
- B2 … El panel de detalle se reconstruye cada 2 s (pierde selección, parpadea, respuestas desordenadas).
- B3 … La tabla se reconstruye cada 2 s (pierde foco).
- B4 … En rangos de 7/30 días, el clic en el gráfico suele dar «No hay datos».
- B5 … «Tiempo activa» solo cuenta minutos entre las que más consumían.
- B6 … La fila «(otros)» encabeza «Qué consumía más».
- B7 … La interfaz no avisa si Vigía se cerró; si `/api/estado` falla al inicio, no reintenta.
- B8 … La primera muestra (tasas en 0) entra al historial.

## Solidez del `unsafe`
- C1 … `from_raw_parts` desde `Table.as_ptr()` de un arreglo de 1 elemento (procedencia de punteros).
- C2 … `ImageName.Buffer` no se valida contra el búfer.
- C3 … Recorrido limitado por capacidad y no por bytes escritos.
- C4 … Posible cuadro «No hay disco» en lectores vacíos (`SetErrorMode`).
- C5 … Bloque de versión `Vec<u8>` sin alineación garantizada.

## Sugerencias
- Caché de letras de unidad en `dispositivo_a_dos`.
- `/api/ahora` envía ~200 KB cada 2 s.
- Esperar el hilo de disco al cerrar.

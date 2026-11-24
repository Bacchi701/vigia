# Revisión de H3 (subagente revisor, sin ver el código antes)

Hallazgos y qué se hizo con cada uno. ✔ corregido · ✗ no se corrige (con motivo).

## Errores reales (colector, historial, servidor)
- A1 ✔ `PRAGMA incremental_vacuum` con `execute_batch` liberaba una sola página: ahora se recorren todas sus filas. Prueba `la_retencion_achica_el_archivo_de_verdad` (el número de páginas del archivo baja a menos de la mitad).
- A2 ✔ Los análisis de disco tienen su propio tope (¼ del tope total, `disco::limitar`); el bucle de retención se corta si borrar historial ya no achica la base; se borran las `apps` huérfanas. Prueba `los_analisis_de_disco_no_obligan_a_borrar_historial`.
- A3 ✔ Un minuto ya guardado se **combina** ponderando por muestras (antes se pisaba). Si el reloj retrocede, no se reabre un minuto cerrado. Pruebas `reinicio_dentro_del_mismo_minuto_combina_en_vez_de_pisar` y `reloj_que_retrocede_no_reabre_un_minuto_cerrado`.
- A4 ✔ `instante_anterior` se actualiza solo si la muestra salió bien; si falla la lectura de red se olvida la referencia.
- A5 ✔ Host/Origin/token se validan **antes** de leer el cuerpo, que solo se lee en POST; cada petición se atiende en su propio hilo (máx. 16). `--detener` ahora informa y devuelve 1 si no se confirma el cierre. Prueba `un_cliente_sin_token_que_no_envia_el_cuerpo_no_bloquea`.
- A6 ✔ Instancia única con un mutex con nombre (`CreateMutexW`, atómico) por carpeta de datos. Prueba `dos_arranques_simultaneos_dejan_una_sola_instancia`.
- A7 ✔ Los números de la URL se limitan a ±2^52. Prueba `parametros_extremos_no_rompen_el_servidor`.

## Interfaz (verificado en el navegador con la versión release)
- B1 ✔ La fila agrupada toma PID, inicio y ruta del proceso más antiguo del grupo: el detalle lo encuentra y la fila queda resaltada.
- B2 ✔ El panel se arma una vez por proceso (los gráficos ya no parpadean), no se reemplaza mientras hay texto seleccionado, y las respuestas viejas se descartan.
- B3 ✔ La tabla no se redibuja con un botón del ratón apretado y devuelve el foco del teclado a la misma fila.
- B4 ✔ `momento` acepta un radio; la interfaz busca dentro del intervalo del punto más cercano. Probado con el rango de 30 días.
- B5 ✔ La columna se llama «Minutos destacada», con una explicación al pasar el ratón.
- B6 ✔ «(otros)» va al final como «Todas las demás».
- B7 ✔ Franja roja «Sin conexión con Vigía» cuando falla la conexión; al arrancar se reintenta cada 3 s.
- B8 ✔ La primera muestra (tasas en 0) solo sirve de referencia y no se registra. ✗ Los procesos nuevos siguen mostrando 0 % en su primera muestra: no hay lectura anterior con qué comparar, y es lo mismo que hace el Administrador de tareas.

## Solidez del `unsafe`
- C1 ✔ El slice de filas se construye desde la dirección del campo completo (`&raw const`).
- C2 ✔ Se comprueba que el nombre del proceso cae dentro del búfer.
- C3 ✔ El recorrido se limita a los bytes que Windows dice haber escrito.
- C4 ✔ `SetErrorMode(SEM_FAILCRITICALERRORS)` al iniciar (solo afecta a este proceso).
- C5 ✔ El bloque de versión se reserva como `u32` (alineado a 4).

## Sugerencias
- ✔ Caché (60 s) de la tabla letra → dispositivo.
- ✗ `/api/ahora` sigue enviando la muestra completa: el consumo se mide en H7 y se decide con ese dato.
- ✗ El hilo de análisis de disco no se espera al cerrar: se le pide cancelar y revisa esa orden en cada carpeta; esperar podría demorar el cierre si una carpeta tarda en listarse.

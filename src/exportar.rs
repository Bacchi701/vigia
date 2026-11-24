//! Exportación del historial a CSV (para abrir en Excel u otra planilla).
//!
//! Formato pensado para Excel en español: separador `;`, coma decimal,
//! codificación UTF-8 con BOM (para que se vean bien las tildes) y fechas en
//! la hora local del equipo.

use crate::error::Resultado;
use crate::historial::OTROS;
use rusqlite::Connection;

/// Qué exportar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    /// Una fila por minuto con los totales del sistema.
    Sistema,
    /// Una fila por minuto y aplicación.
    Aplicaciones,
}

/// Marca de orden de bytes UTF-8: le dice a Excel que el archivo es UTF-8.
const BOM: &str = "\u{FEFF}";

/// Encierra un campo entre comillas si contiene `;`, comillas o saltos de
/// línea (las comillas internas se duplican, como manda el formato CSV).
fn campo(t: &str) -> String {
    if t.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", t.replace('"', "\"\""))
    } else {
        t.to_string()
    }
}

/// Número con coma decimal.
fn num(v: f64, decimales: usize) -> String {
    format!("{v:.decimales$}").replace('.', ",")
}

/// Genera el CSV del rango `[desde, hasta)`. `fecha` convierte una hora
/// Unix (ms) a texto; se recibe como parámetro para poder probarlo.
pub fn csv(
    conn: &Connection,
    tipo: Tipo,
    desde: i64,
    hasta: i64,
    fecha: impl Fn(i64) -> String,
) -> Resultado<String> {
    let mut s = String::from(BOM);
    const MB: f64 = 1_048_576.0;
    match tipo {
        Tipo::Sistema => {
            s.push_str("fecha;cpu_promedio_pct;cpu_maxima_pct;memoria_promedio_mb;memoria_maxima_mb;memoria_total_mb;red_bajada_kb_s;red_subida_kb_s;muestras\r\n");
            let mut st = conn.prepare(
                "SELECT ts, cpu_prom, cpu_max, mem_prom, mem_max, mem_total, red_rx, red_tx, muestras
                 FROM minuto_sistema WHERE ts >= ?1 AND ts < ?2 ORDER BY ts",
            )?;
            let mut filas = st.query([desde, hasta])?;
            while let Some(f) = filas.next()? {
                let linea = [
                    campo(&fecha(f.get(0)?)),
                    num(f.get(1)?, 1),
                    num(f.get(2)?, 1),
                    num(f.get::<_, i64>(3)? as f64 / MB, 0),
                    num(f.get::<_, i64>(4)? as f64 / MB, 0),
                    num(f.get::<_, i64>(5)? as f64 / MB, 0),
                    num(f.get::<_, f64>(6)? / 1024.0, 1),
                    num(f.get::<_, f64>(7)? / 1024.0, 1),
                    f.get::<_, i64>(8)?.to_string(),
                ];
                s.push_str(&linea.join(";"));
                s.push_str("\r\n");
            }
        }
        Tipo::Aplicaciones => {
            s.push_str("fecha;aplicacion;descripcion;empresa;cpu_promedio_pct;cpu_maxima_pct;memoria_promedio_mb;memoria_maxima_mb;lectura_escritura_kb_s;procesos\r\n");
            let mut st = conn.prepare(
                "SELECT m.ts, a.nombre, a.descripcion, a.empresa, m.cpu_prom, m.cpu_max,
                        m.mem_prom, m.mem_max, m.io_prom, m.procesos
                 FROM minuto_app m JOIN apps a ON a.id = m.app_id
                 WHERE m.ts >= ?1 AND m.ts < ?2 ORDER BY m.ts, m.cpu_prom DESC",
            )?;
            let mut filas = st.query([desde, hasta])?;
            while let Some(f) = filas.next()? {
                let nombre: String = f.get(1)?;
                let nombre = if nombre == OTROS {
                    "(todas las demás)".to_string()
                } else {
                    nombre
                };
                let linea = [
                    campo(&fecha(f.get(0)?)),
                    campo(&nombre),
                    campo(&f.get::<_, Option<String>>(2)?.unwrap_or_default()),
                    campo(&f.get::<_, Option<String>>(3)?.unwrap_or_default()),
                    num(f.get(4)?, 2),
                    num(f.get(5)?, 2),
                    num(f.get::<_, i64>(6)? as f64 / MB, 1),
                    num(f.get::<_, i64>(7)? as f64 / MB, 1),
                    num(f.get::<_, f64>(8)? / 1024.0, 1),
                    f.get::<_, i64>(9)?.to_string(),
                ];
                s.push_str(&linea.join(";"));
                s.push_str("\r\n");
            }
        }
    }
    Ok(s)
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::historial::pruebas::{proceso, simular};
    use crate::historial::{Historial, MINUTO_MS, minuto_de};

    #[test]
    fn exporta_sistema_y_aplicaciones() {
        let mut conn = crate::base::en_memoria().unwrap();
        let mut h = Historial::new(10);
        let inicio = minuto_de(1_790_000_000_000);
        simular(&mut conn, &mut h, inicio, 3, |_| {
            let mut p = proceso(1, "raro;nombre.exe", 12.5, 300);
            p.descripcion = Some("Dice \"hola\"".into());
            vec![p]
        });
        let fecha = |ms: i64| format!("F{ms}");
        let s = csv(&conn, Tipo::Sistema, inicio, inicio + 3 * MINUTO_MS, fecha).unwrap();
        let lineas: Vec<&str> = s.trim_end().split("\r\n").collect();
        assert!(lineas[0].starts_with(BOM));
        assert_eq!(lineas.len(), 3); // encabezado + 2 minutos cerrados
        assert!(lineas[1].starts_with(&format!("F{inicio};12,5;12,5;300;")));

        let s = csv(
            &conn,
            Tipo::Aplicaciones,
            inicio,
            inicio + 3 * MINUTO_MS,
            |ms| format!("F{ms}"),
        )
        .unwrap();
        let lineas: Vec<&str> = s.trim_end().split("\r\n").collect();
        assert_eq!(lineas.len(), 3);
        // Campos con ';' y comillas van entre comillas, con comillas dobladas.
        assert!(lineas[1].contains(";\"raro;nombre.exe\";\"Dice \"\"hola\"\"\";"));
        assert!(lineas[1].contains(";12,50;12,50;300,0;"));
    }
}

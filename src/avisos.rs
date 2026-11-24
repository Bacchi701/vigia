//! Avisos: se guardan en la tabla `avisos` y se muestran en la interfaz.

use crate::error::Resultado;
use rusqlite::{Connection, params};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Aviso {
    pub id: i64,
    pub ts: i64,
    /// Identificador del tipo: "inicio_nuevo", "puerto_nuevo", "memoria_alta"...
    pub tipo: String,
    pub titulo: String,
    pub detalle: String,
    pub visto: bool,
}

/// Guarda un aviso y devuelve su id.
pub fn registrar(
    conn: &Connection,
    ts: i64,
    tipo: &str,
    titulo: &str,
    detalle: &str,
) -> Resultado<i64> {
    conn.execute(
        "INSERT INTO avisos(ts, tipo, titulo, detalle) VALUES (?1, ?2, ?3, ?4)",
        params![ts, tipo, titulo, detalle],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Los avisos más recientes primero.
pub fn listar(conn: &Connection, limite: usize) -> Resultado<Vec<Aviso>> {
    let mut st = conn.prepare(
        "SELECT id, ts, tipo, titulo, detalle, visto FROM avisos ORDER BY id DESC LIMIT ?1",
    )?;
    let filas = st.query_map([limite as i64], |f| {
        Ok(Aviso {
            id: f.get(0)?,
            ts: f.get(1)?,
            tipo: f.get(2)?,
            titulo: f.get(3)?,
            detalle: f.get(4)?,
            visto: f.get(5)?,
        })
    })?;
    Ok(filas.collect::<Result<Vec<_>, _>>()?)
}

pub fn sin_ver(conn: &Connection) -> Resultado<i64> {
    Ok(
        conn.query_row("SELECT COUNT(*) FROM avisos WHERE visto = 0", [], |f| {
            f.get(0)
        })?,
    )
}

pub fn marcar_vistos(conn: &Connection) -> Resultado<usize> {
    Ok(conn.execute("UPDATE avisos SET visto = 1 WHERE visto = 0", [])?)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn registra_lista_y_marca() {
        let c = crate::base::en_memoria().unwrap();
        registrar(&c, 1, "a", "Uno", "d").unwrap();
        registrar(&c, 2, "b", "Dos", "d").unwrap();
        let l = listar(&c, 10).unwrap();
        assert_eq!(l[0].titulo, "Dos");
        assert_eq!(sin_ver(&c).unwrap(), 2);
        assert_eq!(marcar_vistos(&c).unwrap(), 2);
        assert_eq!(sin_ver(&c).unwrap(), 0);
    }
}

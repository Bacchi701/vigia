//! Base de datos SQLite: apertura, esquema y configuración clave/valor.
//!
//! Todas las tablas del programa se crean aquí, en un solo lugar, para que
//! sea fácil ver qué se guarda en disco.

use crate::error::Resultado;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// Esquema completo. `IF NOT EXISTS` permite ejecutarlo en cada arranque.
const ESQUEMA: &str = "
-- Una fila por aplicación (agrupada por nombre de ejecutable).
CREATE TABLE IF NOT EXISTS apps (
    id          INTEGER PRIMARY KEY,
    nombre      TEXT NOT NULL UNIQUE,
    ruta        TEXT,
    descripcion TEXT,
    empresa     TEXT
);

-- Resumen del sistema por minuto. ts = inicio del minuto (ms Unix, UTC).
CREATE TABLE IF NOT EXISTS minuto_sistema (
    ts        INTEGER PRIMARY KEY,
    cpu_prom  REAL NOT NULL,
    cpu_max   REAL NOT NULL,
    mem_prom  INTEGER NOT NULL,
    mem_max   INTEGER NOT NULL,
    mem_total INTEGER NOT NULL,
    red_rx    REAL NOT NULL,   -- bytes/s promedio
    red_tx    REAL NOT NULL,
    muestras  INTEGER NOT NULL
);

-- Resumen por aplicación y minuto (solo las que más consumieron, más una
-- fila '(otros)' con el resto). WITHOUT ROWID ahorra espacio porque la
-- clave primaria ya identifica la fila.
CREATE TABLE IF NOT EXISTS minuto_app (
    ts        INTEGER NOT NULL,
    app_id    INTEGER NOT NULL REFERENCES apps(id),
    cpu_prom  REAL NOT NULL,
    cpu_max   REAL NOT NULL,
    mem_prom  INTEGER NOT NULL,
    mem_max   INTEGER NOT NULL,
    io_prom   REAL NOT NULL,   -- bytes/s promedio (lectura + escritura)
    procesos  INTEGER NOT NULL,
    PRIMARY KEY (ts, app_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS minuto_app_por_app ON minuto_app(app_id, ts);

-- Configuración y estado varios, como texto JSON.
CREATE TABLE IF NOT EXISTS config (
    clave TEXT PRIMARY KEY,
    valor TEXT NOT NULL
);

-- Análisis de disco guardados (resultado completo en JSON).
CREATE TABLE IF NOT EXISTS analisis_disco (
    id        INTEGER PRIMARY KEY,
    raiz      TEXT NOT NULL,
    inicio    INTEGER NOT NULL,
    fin       INTEGER NOT NULL,
    total     INTEGER NOT NULL,
    archivos  INTEGER NOT NULL,
    completo  INTEGER NOT NULL,
    datos     TEXT NOT NULL
);

-- Avisos generados.
CREATE TABLE IF NOT EXISTS avisos (
    id      INTEGER PRIMARY KEY,
    ts      INTEGER NOT NULL,
    tipo    TEXT NOT NULL,
    titulo  TEXT NOT NULL,
    detalle TEXT NOT NULL,
    visto   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS avisos_por_ts ON avisos(ts);
";

/// Abre (o crea) la base de datos en un archivo.
pub fn abrir(ruta: &Path) -> Resultado<Connection> {
    if let Some(dir) = ruta.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(ruta)?;
    preparar(&conn)?;
    Ok(conn)
}

/// Base en memoria, para pruebas: no escribe nada en disco.
pub fn en_memoria() -> Resultado<Connection> {
    let conn = Connection::open_in_memory()?;
    preparar(&conn)?;
    Ok(conn)
}

fn preparar(conn: &Connection) -> Resultado<()> {
    // auto_vacuum=INCREMENTAL debe fijarse antes de crear tablas: permite
    // devolver al disco el espacio de filas borradas sin reescribir todo el
    // archivo (lo hace `PRAGMA incremental_vacuum`).
    conn.execute_batch(
        "PRAGMA auto_vacuum = INCREMENTAL;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;
    conn.execute_batch(ESQUEMA)?;
    Ok(())
}

/// Lee un valor de configuración (JSON) o `None` si no existe.
pub fn leer_config<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    clave: &str,
) -> Resultado<Option<T>> {
    let texto: Option<String> = conn
        .query_row("SELECT valor FROM config WHERE clave = ?1", [clave], |f| {
            f.get(0)
        })
        .optional()?;
    match texto {
        Some(t) => Ok(Some(serde_json::from_str(&t)?)),
        None => Ok(None),
    }
}

/// Guarda un valor de configuración como JSON.
pub fn guardar_config<T: serde::Serialize>(
    conn: &Connection,
    clave: &str,
    valor: &T,
) -> Resultado<()> {
    let texto = serde_json::to_string(valor)?;
    conn.execute(
        "INSERT INTO config(clave, valor) VALUES (?1, ?2)
         ON CONFLICT(clave) DO UPDATE SET valor = excluded.valor",
        params![clave, texto],
    )?;
    Ok(())
}

/// Bytes realmente ocupados por datos (sin contar páginas libres).
pub fn bytes_usados(conn: &Connection) -> Resultado<u64> {
    let paginas: i64 = conn.query_row("PRAGMA page_count", [], |f| f.get(0))?;
    let libres: i64 = conn.query_row("PRAGMA freelist_count", [], |f| f.get(0))?;
    let tam: i64 = conn.query_row("PRAGMA page_size", [], |f| f.get(0))?;
    Ok(((paginas - libres).max(0) * tam) as u64)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn config_ida_y_vuelta() {
        let c = en_memoria().unwrap();
        assert_eq!(leer_config::<i32>(&c, "x").unwrap(), None);
        guardar_config(&c, "x", &42).unwrap();
        guardar_config(&c, "x", &43).unwrap();
        assert_eq!(leer_config::<i32>(&c, "x").unwrap(), Some(43));
        assert!(bytes_usados(&c).unwrap() > 0);
    }
}

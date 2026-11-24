//! Análisis de disco bajo demanda: qué carpetas, archivos y tipos de archivo
//! ocupan más espacio, y qué creció desde el análisis anterior.
//!
//! **Nunca borra nada.** Solo lee nombres y tamaños con
//! [`crate::win::archivos::listar`].
//!
//! El recorrido es iterativo (una pila propia en vez de recursión): una
//! carpeta muy profunda no puede desbordar la pila del hilo.

use crate::error::{Resultado, error};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Cuántos archivos grandes se guardan.
const ARCHIVOS_GRANDES: usize = 100;
/// Máximo de carpetas que se guardan en el árbol resumido.
const MAX_NODOS: usize = 4000;
/// Profundidad máxima del árbol resumido.
const MAX_PROFUNDIDAD: usize = 12;
/// Análisis guardados por cada raíz.
const ANALISIS_POR_RAIZ: i64 = 10;

/// Progreso de un análisis en curso (lo lee la interfaz).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Progreso {
    pub en_curso: bool,
    pub raiz: String,
    pub carpetas: u64,
    pub archivos: u64,
    pub bytes: u64,
    pub actual: String,
    pub errores: u64,
    pub inicio_ms: i64,
    /// Mensaje del último error que detuvo el análisis, si hubo.
    pub fallo: Option<String>,
    /// Id del último análisis guardado.
    pub ultimo_id: Option<i64>,
}

/// Una carpeta del árbol resumido.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Nodo {
    pub nombre: String,
    /// Tamaño total (esta carpeta y todo lo que contiene).
    pub tamano: u64,
    pub archivos: u64,
    /// Tamaño de las subcarpetas demasiado pequeñas para listarlas.
    pub otros: u64,
    pub hijos: Vec<Nodo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchivoGrande {
    pub ruta: String,
    pub tamano: u64,
    pub modificado_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tipo {
    /// Extensión en minúsculas, sin punto ("" = sin extensión).
    pub extension: String,
    pub bytes: u64,
    pub archivos: u64,
}

/// Resultado completo de un análisis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analisis {
    #[serde(default)]
    pub id: i64,
    pub raiz: String,
    pub inicio_ms: i64,
    pub fin_ms: i64,
    pub completo: bool,
    pub total: u64,
    pub archivos: u64,
    pub carpetas: u64,
    pub errores: u64,
    /// Carpetas que eran enlaces o uniones y no se recorrieron.
    pub enlaces_omitidos: u64,
    /// Bytes de archivos que están solo en la nube (no ocupan este disco).
    pub en_la_nube: u64,
    pub arbol: Nodo,
    pub grandes: Vec<ArchivoGrande>,
    pub tipos: Vec<Tipo>,
}

/// Carpeta durante el recorrido.
struct Carpeta {
    padre: Option<usize>,
    nombre: String,
    propio: u64,
    archivos_propios: u64,
    total: u64,
    archivos_total: u64,
}

/// Quita la barra final, salvo en la raíz de una unidad ("C:\").
pub fn normalizar_raiz(ruta: &str) -> Resultado<String> {
    let r = ruta.trim().trim_matches('"').replace('/', "\\");
    let r = if r.len() == 2 && r.ends_with(':') {
        format!("{r}\\")
    } else {
        r
    };
    let p = std::path::Path::new(&r);
    if !p.is_absolute() {
        return Err(error("La ruta debe ser completa, por ejemplo C:\\Users"));
    }
    if !p.is_dir() {
        return Err(error(format!("No existe la carpeta {r}")));
    }
    if r.len() > 3 {
        Ok(r.trim_end_matches('\\').to_string())
    } else {
        Ok(r)
    }
}

fn unir(carpeta: &str, nombre: &str) -> String {
    if carpeta.ends_with('\\') {
        format!("{carpeta}{nombre}")
    } else {
        format!("{carpeta}\\{nombre}")
    }
}

/// Recorre `raiz` (ya normalizada). Revisa `cancelar` en cada carpeta y
/// actualiza `progreso` cada ~150 ms. Si se cancela, devuelve lo recorrido
/// hasta ahí con `completo = false`.
pub fn analizar(
    raiz: &str,
    ahora_ms: impl Fn() -> i64,
    cancelar: &AtomicBool,
    progreso: &Mutex<Progreso>,
) -> Analisis {
    let inicio_ms = ahora_ms();
    let mut carpetas = vec![Carpeta {
        padre: None,
        nombre: raiz.to_string(),
        propio: 0,
        archivos_propios: 0,
        total: 0,
        archivos_total: 0,
    }];
    let mut pila: Vec<(usize, String)> = vec![(0, raiz.to_string())];
    // Montículo de mínimos con los archivos más grandes: el tope es el más
    // chico de los guardados, así se descarta rápido lo que no entra.
    let mut grandes: BinaryHeap<Reverse<(u64, String, Option<i64>)>> = BinaryHeap::new();
    let mut tipos: HashMap<String, (u64, u64)> = HashMap::new();
    let (mut archivos, mut bytes, mut errores, mut enlaces, mut nube) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut completo = true;
    let mut ultimo_aviso = Instant::now();

    while let Some((indice, ruta)) = pila.pop() {
        if cancelar.load(Ordering::Relaxed) {
            completo = false;
            break;
        }
        let entradas = match crate::win::archivos::listar(&ruta) {
            Ok(e) => e,
            Err(_) => {
                // Sin permiso (p. ej. "System Volume Information") o carpeta
                // que desapareció: se cuenta y se sigue.
                errores += 1;
                continue;
            }
        };
        for e in entradas {
            if e.es_carpeta {
                if e.es_enlace {
                    enlaces += 1;
                    continue;
                }
                carpetas.push(Carpeta {
                    padre: Some(indice),
                    nombre: e.nombre.clone(),
                    propio: 0,
                    archivos_propios: 0,
                    total: 0,
                    archivos_total: 0,
                });
                pila.push((carpetas.len() - 1, unir(&ruta, &e.nombre)));
                continue;
            }
            if e.en_la_nube {
                nube += e.tamano;
                continue;
            }
            archivos += 1;
            bytes += e.tamano;
            let c = &mut carpetas[indice];
            c.propio += e.tamano;
            c.archivos_propios += 1;
            let ext = std::path::Path::new(&e.nombre)
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let t = tipos.entry(ext).or_default();
            t.0 += e.tamano;
            t.1 += 1;
            let entra = grandes.len() < ARCHIVOS_GRANDES
                || grandes.peek().is_some_and(|Reverse(m)| e.tamano > m.0);
            if entra {
                grandes.push(Reverse((e.tamano, unir(&ruta, &e.nombre), e.modificado_ms)));
                if grandes.len() > ARCHIVOS_GRANDES {
                    grandes.pop();
                }
            }
        }
        if ultimo_aviso.elapsed() >= Duration::from_millis(150) {
            ultimo_aviso = Instant::now();
            let mut p = crate::app::bloquear(progreso);
            p.carpetas = carpetas.len() as u64;
            p.archivos = archivos;
            p.bytes = bytes;
            p.errores = errores;
            p.actual = ruta;
        }
    }

    // Totales de abajo hacia arriba: cada carpeta se creó después que su
    // padre, así que recorrer al revés suma primero los hijos.
    for i in (0..carpetas.len()).rev() {
        let (total, n) = (
            carpetas[i].total + carpetas[i].propio,
            carpetas[i].archivos_total + carpetas[i].archivos_propios,
        );
        carpetas[i].total = total;
        carpetas[i].archivos_total = n;
        if let Some(p) = carpetas[i].padre {
            carpetas[p].total += total;
            carpetas[p].archivos_total += n;
        }
    }

    let arbol = resumir_arbol(&carpetas);
    let mut grandes: Vec<ArchivoGrande> = grandes
        .into_iter()
        .map(|Reverse((tamano, ruta, modificado_ms))| ArchivoGrande {
            ruta,
            tamano,
            modificado_ms,
        })
        .collect();
    grandes.sort_by_key(|a| Reverse(a.tamano));
    let mut tipos: Vec<Tipo> = tipos
        .into_iter()
        .map(|(extension, (bytes, archivos))| Tipo {
            extension,
            bytes,
            archivos,
        })
        .collect();
    tipos.sort_by_key(|t| Reverse(t.bytes));
    tipos.truncate(60);

    Analisis {
        id: 0,
        raiz: raiz.to_string(),
        inicio_ms,
        fin_ms: ahora_ms(),
        completo,
        total: bytes,
        archivos,
        carpetas: carpetas.len() as u64,
        errores,
        enlaces_omitidos: enlaces,
        en_la_nube: nube,
        arbol,
        grandes,
        tipos,
    }
}

/// Construye el árbol resumido: solo las carpetas que pesan al menos el
/// 0,1 % del total (y como máximo `MAX_NODOS`), ordenadas por tamaño.
fn resumir_arbol(carpetas: &[Carpeta]) -> Nodo {
    let mut hijos: Vec<Vec<usize>> = vec![Vec::new(); carpetas.len()];
    for (i, c) in carpetas.iter().enumerate() {
        if let Some(p) = c.padre {
            hijos[p].push(i);
        }
    }
    let umbral = (carpetas[0].total / 1000).max(1);
    let mut restantes = MAX_NODOS;
    construir(0, 0, carpetas, &hijos, umbral, &mut restantes)
}

fn construir(
    i: usize,
    profundidad: usize,
    carpetas: &[Carpeta],
    hijos: &[Vec<usize>],
    umbral: u64,
    restantes: &mut usize,
) -> Nodo {
    let c = &carpetas[i];
    let mut nodo = Nodo {
        nombre: c.nombre.clone(),
        tamano: c.total,
        archivos: c.archivos_total,
        otros: 0,
        hijos: Vec::new(),
    };
    let mut lista: Vec<usize> = hijos[i].clone();
    lista.sort_by_key(|&h| Reverse(carpetas[h].total));
    for h in lista {
        if profundidad < MAX_PROFUNDIDAD && carpetas[h].total >= umbral && *restantes > 0 {
            *restantes -= 1;
            nodo.hijos.push(construir(
                h,
                profundidad + 1,
                carpetas,
                hijos,
                umbral,
                restantes,
            ));
        } else {
            nodo.otros += carpetas[h].total;
        }
    }
    nodo
}

/// Clave para comparar raíces sin importar mayúsculas.
fn clave_raiz(raiz: &str) -> String {
    raiz.trim_end_matches('\\').to_lowercase()
}

/// Guarda un análisis y deja solo los últimos `ANALISIS_POR_RAIZ` de esa raíz.
pub fn guardar(conn: &Connection, a: &Analisis) -> Resultado<i64> {
    let datos = serde_json::to_string(a)?;
    conn.execute(
        "INSERT INTO analisis_disco(raiz, inicio, fin, total, archivos, completo, datos)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            clave_raiz(&a.raiz),
            a.inicio_ms,
            a.fin_ms,
            a.total as i64,
            a.archivos as i64,
            a.completo,
            datos
        ],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM analisis_disco WHERE raiz = ?1 AND id NOT IN
           (SELECT id FROM analisis_disco WHERE raiz = ?1 ORDER BY id DESC LIMIT ?2)",
        params![clave_raiz(&a.raiz), ANALISIS_POR_RAIZ],
    )?;
    Ok(id)
}

/// Resumen de un análisis guardado (para la lista).
#[derive(Debug, Clone, Serialize)]
pub struct Guardado {
    pub id: i64,
    pub raiz: String,
    pub fin_ms: i64,
    pub total: u64,
    pub archivos: u64,
    pub completo: bool,
}

pub fn lista(conn: &Connection) -> Resultado<Vec<Guardado>> {
    let mut st = conn.prepare(
        "SELECT id, json_extract(datos, '$.raiz'), fin, total, archivos, completo
         FROM analisis_disco ORDER BY id DESC LIMIT 50",
    )?;
    let filas = st.query_map([], |f| {
        Ok(Guardado {
            id: f.get(0)?,
            raiz: f.get(1)?,
            fin_ms: f.get(2)?,
            total: f.get::<_, i64>(3)? as u64,
            archivos: f.get::<_, i64>(4)? as u64,
            completo: f.get(5)?,
        })
    })?;
    Ok(filas.collect::<Result<Vec<_>, _>>()?)
}

/// Lee un análisis por id (o el más reciente si `id` es `None`).
pub fn leer(conn: &Connection, id: Option<i64>) -> Resultado<Option<Analisis>> {
    let fila: Option<(i64, String)> = match id {
        Some(id) => conn
            .query_row(
                "SELECT id, datos FROM analisis_disco WHERE id = ?1",
                [id],
                |f| Ok((f.get(0)?, f.get(1)?)),
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT id, datos FROM analisis_disco ORDER BY id DESC LIMIT 1",
                [],
                |f| Ok((f.get(0)?, f.get(1)?)),
            )
            .optional()?,
    };
    match fila {
        Some((id, datos)) => {
            let mut a: Analisis = serde_json::from_str(&datos)?;
            a.id = id;
            Ok(Some(a))
        }
        None => Ok(None),
    }
}

/// El análisis completo anterior a `a` de la misma raíz.
pub fn anterior(conn: &Connection, a: &Analisis) -> Resultado<Option<Analisis>> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM analisis_disco WHERE raiz = ?1 AND id < ?2 AND completo = 1
             ORDER BY id DESC LIMIT 1",
            params![clave_raiz(&a.raiz), a.id],
            |f| f.get(0),
        )
        .optional()?;
    match id {
        Some(id) => leer(conn, Some(id)),
        None => Ok(None),
    }
}

/// Una carpeta que cambió de tamaño entre dos análisis.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Cambio {
    pub ruta: String,
    pub antes: u64,
    pub ahora: u64,
    /// ahora − antes (negativo si se achicó).
    pub diferencia: i64,
}

/// Compara dos análisis de la misma raíz carpeta por carpeta (solo las
/// carpetas del árbol resumido). Devuelve las que más crecieron primero.
pub fn comparar(antes: &Analisis, ahora: &Analisis) -> Vec<Cambio> {
    fn aplanar(n: &Nodo, ruta: &str, salida: &mut HashMap<String, (String, u64)>) {
        for h in &n.hijos {
            let r = unir(ruta, &h.nombre);
            salida.insert(r.to_lowercase(), (r.clone(), h.tamano));
            aplanar(h, &r, salida);
        }
    }
    let mut a = HashMap::new();
    let mut b = HashMap::new();
    aplanar(&antes.arbol, &antes.raiz, &mut a);
    aplanar(&ahora.arbol, &ahora.raiz, &mut b);
    let mut cambios: Vec<Cambio> = Vec::new();
    for (clave, (ruta, tam)) in &b {
        let previo = a.get(clave).map(|x| x.1).unwrap_or(0);
        if previo != *tam {
            cambios.push(Cambio {
                ruta: ruta.clone(),
                antes: previo,
                ahora: *tam,
                diferencia: *tam as i64 - previo as i64,
            });
        }
    }
    for (clave, (ruta, tam)) in &a {
        if !b.contains_key(clave) {
            cambios.push(Cambio {
                ruta: ruta.clone(),
                antes: *tam,
                ahora: 0,
                diferencia: -(*tam as i64),
            });
        }
    }
    cambios.sort_by_key(|c| Reverse(c.diferencia));
    cambios
}

/// ¿`ruta` aparece en el análisis (como archivo grande o carpeta del
/// árbol)? Solo esas rutas se pueden abrir en el Explorador desde la
/// interfaz: así la API no sirve para abrir rutas arbitrarias.
pub fn contiene_ruta(a: &Analisis, ruta: &str) -> bool {
    let buscada = ruta.to_lowercase();
    if a.grandes.iter().any(|g| g.ruta.to_lowercase() == buscada) {
        return true;
    }
    fn buscar(n: &Nodo, actual: &str, buscada: &str) -> bool {
        if actual.to_lowercase() == buscada {
            return true;
        }
        n.hijos.iter().any(|h| {
            let r = unir(actual, &h.nombre);
            buscada.starts_with(&r.to_lowercase()) && buscar(h, &r, buscada)
        })
    }
    buscar(&a.arbol, &a.raiz, &buscada)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn nodo(nombre: &str, tamano: u64, hijos: Vec<Nodo>) -> Nodo {
        Nodo {
            nombre: nombre.into(),
            tamano,
            archivos: 1,
            otros: 0,
            hijos,
        }
    }

    fn analisis(arbol: Nodo) -> Analisis {
        Analisis {
            id: 0,
            raiz: "C:\\x".into(),
            inicio_ms: 0,
            fin_ms: 0,
            completo: true,
            total: arbol.tamano,
            archivos: 0,
            carpetas: 0,
            errores: 0,
            enlaces_omitidos: 0,
            en_la_nube: 0,
            arbol,
            grandes: vec![ArchivoGrande {
                ruta: "C:\\x\\a\\grande.bin".into(),
                tamano: 50,
                modificado_ms: None,
            }],
            tipos: vec![],
        }
    }

    #[test]
    fn compara_carpetas() {
        let antes = analisis(nodo(
            "C:\\x",
            100,
            vec![nodo("a", 60, vec![]), nodo("b", 40, vec![])],
        ));
        let ahora = analisis(nodo(
            "C:\\x",
            190,
            vec![nodo("a", 150, vec![]), nodo("c", 40, vec![])],
        ));
        let c = comparar(&antes, &ahora);
        assert_eq!(c[0].ruta, "C:\\x\\a");
        assert_eq!(c[0].diferencia, 90);
        assert!(c.iter().any(|x| x.ruta == "C:\\x\\c" && x.diferencia == 40));
        assert_eq!(c.last().unwrap().ruta, "C:\\x\\b");
        assert_eq!(c.last().unwrap().diferencia, -40);
    }

    #[test]
    fn solo_rutas_del_analisis() {
        let a = analisis(nodo(
            "C:\\x",
            100,
            vec![nodo("a", 60, vec![nodo("sub", 10, vec![])])],
        ));
        assert!(contiene_ruta(&a, "C:\\x\\a\\grande.bin"));
        assert!(contiene_ruta(&a, "c:\\X\\A\\sub"));
        assert!(contiene_ruta(&a, "C:\\x"));
        assert!(!contiene_ruta(&a, "C:\\Windows\\System32"));
        assert!(!contiene_ruta(&a, "C:\\x\\a\\otra"));
    }

    #[test]
    fn normaliza_raices() {
        assert!(normalizar_raiz("relativa\\x").is_err());
        assert!(normalizar_raiz("C:\\no\\existe\\de\\verdad\\123").is_err());
        assert_eq!(normalizar_raiz("c:").unwrap(), "c:\\");
        let aqui = env!("CARGO_MANIFEST_DIR");
        assert_eq!(normalizar_raiz(&format!("{aqui}\\")).unwrap(), aqui);
    }

    #[test]
    fn guarda_lista_y_conserva_solo_los_ultimos() {
        let conn = crate::base::en_memoria().unwrap();
        let mut ultimo = 0;
        for i in 0..12 {
            let mut a = analisis(nodo("C:\\x", i, vec![]));
            a.fin_ms = i as i64;
            ultimo = guardar(&conn, &a).unwrap();
        }
        let l = lista(&conn).unwrap();
        assert_eq!(l.len(), 10);
        assert_eq!(l[0].id, ultimo);
        let a = leer(&conn, None).unwrap().unwrap();
        assert_eq!(a.id, ultimo);
        assert_eq!(a.total, 11);
        let previo = anterior(&conn, &a).unwrap().unwrap();
        assert_eq!(previo.total, 10);
    }
}

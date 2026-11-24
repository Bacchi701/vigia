//! Historial: muestras en vivo en memoria y resúmenes por minuto en SQLite.
//!
//! - **En vivo**: los últimos minutos, muestra a muestra, solo en memoria
//!   (para los gráficos de la pantalla «Ahora»).
//! - **Por minuto**: al cerrar cada minuto se calcula un resumen del sistema
//!   y de cada aplicación, y se guarda en SQLite (pantallas «Historial» y
//!   «Aplicaciones»). Una *aplicación* es el conjunto de procesos con el
//!   mismo nombre de ejecutable: los 20 procesos `chrome.exe` suman como uno.
//!
//! Ninguna función de este módulo lee el reloj: la hora llega siempre como
//! parámetro (`ts`, `ahora_ms`). Así las pruebas simulan días enteros en
//! milisegundos.

use crate::base;
use crate::colector::Muestra;
use crate::error::Resultado;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};

pub const MINUTO_MS: i64 = 60_000;
pub const DIA_MS: i64 = 86_400_000;

/// Nombre de la fila que junta a las aplicaciones que no entraron en el
/// resumen de un minuto.
pub const OTROS: &str = "(otros)";

/// Cuántas aplicaciones se guardan por minuto en cada categoría.
const TOP_CPU: usize = 12;
const TOP_MEMORIA: usize = 12;
const TOP_IO: usize = 5;

/// Un punto de los gráficos en vivo.
#[derive(Debug, Clone, Serialize)]
pub struct PuntoVivo {
    pub ts: i64,
    pub cpu: f64,
    pub mem_usada: u64,
    pub mem_total: u64,
    pub red_rx_bps: f64,
    pub red_tx_bps: f64,
    /// E/S total de todos los procesos, bytes/s.
    pub io_bps: f64,
}

/// Punto del historial reciente de un proceso (para su panel de detalle).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PuntoProceso {
    pub ts: i64,
    pub cpu: f64,
    pub memoria: u64,
}

/// Resumen del sistema en un minuto.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MinutoSistema {
    pub ts: i64,
    pub cpu_prom: f64,
    pub cpu_max: f64,
    pub mem_prom: u64,
    pub mem_max: u64,
    pub mem_total: u64,
    pub red_rx: f64,
    pub red_tx: f64,
    pub muestras: u32,
}

/// Resumen de una aplicación en un minuto.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MinutoApp {
    pub nombre: String,
    pub ruta: Option<String>,
    pub descripcion: Option<String>,
    pub empresa: Option<String>,
    pub cpu_prom: f64,
    pub cpu_max: f64,
    pub mem_prom: u64,
    pub mem_max: u64,
    pub io_prom: f64,
    pub procesos: u32,
}

/// Lo que se guarda al cerrar un minuto.
#[derive(Debug, Clone)]
pub struct ResumenMinuto {
    pub sistema: MinutoSistema,
    pub apps: Vec<MinutoApp>,
}

#[derive(Default)]
struct AcumApp {
    cpu_suma: f64,
    cpu_max: f64,
    mem_suma: u64,
    mem_max: u64,
    io_suma: f64,
    procesos_max: u32,
    ruta: Option<String>,
    descripcion: Option<String>,
    empresa: Option<String>,
}

/// Va sumando las muestras del minuto en curso.
struct Acumulador {
    minuto: i64,
    muestras: u32,
    cpu_suma: f64,
    cpu_max: f64,
    mem_suma: u64,
    mem_max: u64,
    mem_total: u64,
    rx_suma: f64,
    tx_suma: f64,
    apps: HashMap<String, AcumApp>,
}

/// Clave con la que se agrupan procesos en una aplicación.
pub fn clave_app(nombre: &str) -> String {
    let n = nombre.trim().to_lowercase();
    if n.is_empty() {
        "(sin nombre)".to_string()
    } else {
        n
    }
}

/// Inicio del minuto que contiene `ts`.
pub fn minuto_de(ts: i64) -> i64 {
    ts.div_euclid(MINUTO_MS) * MINUTO_MS
}

/// Estado en memoria del historial.
pub struct Historial {
    capacidad: usize,
    vivo: VecDeque<PuntoVivo>,
    procesos: HashMap<(u32, i64), VecDeque<PuntoProceso>>,
    acum: Option<Acumulador>,
}

impl Historial {
    /// `capacidad`: cuántas muestras en vivo conservar (p. ej. 10 minutos
    /// a una muestra cada 2 s = 300).
    pub fn new(capacidad: usize) -> Self {
        Self {
            capacidad: capacidad.max(1),
            vivo: VecDeque::with_capacity(capacidad),
            procesos: HashMap::new(),
            acum: None,
        }
    }

    pub fn vivo(&self) -> impl Iterator<Item = &PuntoVivo> {
        self.vivo.iter()
    }

    /// Historial reciente de un proceso (por PID y hora de inicio).
    pub fn proceso(&self, pid: u32, inicio_ms: i64) -> Vec<PuntoProceso> {
        self.procesos
            .get(&(pid, inicio_ms))
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Agrega una muestra. Si con ella termina un minuto, devuelve el
    /// resumen de ese minuto para guardarlo.
    pub fn registrar(&mut self, m: &Muestra) -> Option<ResumenMinuto> {
        // --- En vivo ---
        let io_bps = m
            .procesos
            .iter()
            .map(|p| p.io_lectura_bps + p.io_escritura_bps)
            .sum();
        if self.vivo.len() == self.capacidad {
            self.vivo.pop_front();
        }
        self.vivo.push_back(PuntoVivo {
            ts: m.ts,
            cpu: m.sistema.cpu,
            mem_usada: m.sistema.mem_usada,
            mem_total: m.sistema.mem_total,
            red_rx_bps: m.sistema.red_rx_bps,
            red_tx_bps: m.sistema.red_tx_bps,
            io_bps,
        });
        let mut vistos = HashSet::with_capacity(m.procesos.len());
        for p in &m.procesos {
            let clave = (p.pid, p.inicio_ms.unwrap_or(0));
            vistos.insert(clave);
            let serie = self.procesos.entry(clave).or_default();
            if serie.len() == self.capacidad {
                serie.pop_front();
            }
            serie.push_back(PuntoProceso {
                ts: m.ts,
                cpu: p.cpu,
                memoria: p.memoria,
            });
        }
        self.procesos.retain(|k, _| vistos.contains(k));

        // --- Por minuto ---
        let minuto = minuto_de(m.ts);
        let cerrado = match &self.acum {
            Some(a) if a.minuto != minuto => self.acum.take().map(finalizar),
            _ => None,
        };
        let acum = self.acum.get_or_insert_with(|| Acumulador {
            minuto,
            muestras: 0,
            cpu_suma: 0.0,
            cpu_max: 0.0,
            mem_suma: 0,
            mem_max: 0,
            mem_total: 0,
            rx_suma: 0.0,
            tx_suma: 0.0,
            apps: HashMap::new(),
        });
        sumar(acum, m);
        cerrado
    }

    /// Cierra el minuto en curso aunque no haya terminado (al apagar).
    pub fn cerrar(&mut self) -> Option<ResumenMinuto> {
        self.acum.take().map(finalizar)
    }
}

fn sumar(a: &mut Acumulador, m: &Muestra) {
    a.muestras += 1;
    a.cpu_suma += m.sistema.cpu;
    a.cpu_max = a.cpu_max.max(m.sistema.cpu);
    a.mem_suma += m.sistema.mem_usada;
    a.mem_max = a.mem_max.max(m.sistema.mem_usada);
    a.mem_total = m.sistema.mem_total;
    a.rx_suma += m.sistema.red_rx_bps;
    a.tx_suma += m.sistema.red_tx_bps;

    // Primero sumamos los procesos de esta muestra por aplicación...
    struct Suma<'a> {
        cpu: f64,
        mem: u64,
        io: f64,
        n: u32,
        p: &'a crate::colector::Proceso,
    }
    let mut por_app: HashMap<String, Suma> = HashMap::new();
    for p in &m.procesos {
        let s = por_app.entry(clave_app(&p.nombre)).or_insert(Suma {
            cpu: 0.0,
            mem: 0,
            io: 0.0,
            n: 0,
            p,
        });
        s.cpu += p.cpu;
        s.mem += p.memoria;
        s.io += p.io_lectura_bps + p.io_escritura_bps;
        s.n += 1;
        if s.p.ruta.is_none() && p.ruta.is_some() {
            s.p = p;
        }
    }
    // ...y luego lo acumulamos en el minuto.
    for (clave, s) in por_app {
        let app = a.apps.entry(clave).or_default();
        app.cpu_suma += s.cpu;
        app.cpu_max = app.cpu_max.max(s.cpu);
        app.mem_suma += s.mem;
        app.mem_max = app.mem_max.max(s.mem);
        app.io_suma += s.io;
        app.procesos_max = app.procesos_max.max(s.n);
        if app.ruta.is_none() {
            app.ruta = s.p.ruta.clone();
            app.descripcion = s.p.descripcion.clone();
            app.empresa = s.p.empresa.clone();
        }
    }
}

fn finalizar(a: Acumulador) -> ResumenMinuto {
    let n = f64::from(a.muestras.max(1));
    let n_u = u64::from(a.muestras.max(1));
    let sistema = MinutoSistema {
        ts: a.minuto,
        cpu_prom: a.cpu_suma / n,
        cpu_max: a.cpu_max,
        mem_prom: a.mem_suma / n_u,
        mem_max: a.mem_max,
        mem_total: a.mem_total,
        red_rx: a.rx_suma / n,
        red_tx: a.tx_suma / n,
        muestras: a.muestras,
    };
    // Promedios del minuto. Una app que apareció solo en algunas muestras
    // promedia como si en las otras hubiera usado 0: así los promedios de
    // todas las apps suman el total del sistema.
    let mut apps: Vec<MinutoApp> = a
        .apps
        .into_iter()
        .map(|(nombre, x)| MinutoApp {
            nombre,
            ruta: x.ruta,
            descripcion: x.descripcion,
            empresa: x.empresa,
            cpu_prom: x.cpu_suma / n,
            cpu_max: x.cpu_max,
            mem_prom: x.mem_suma / n_u,
            mem_max: x.mem_max,
            io_prom: x.io_suma / n,
            procesos: x.procesos_max,
        })
        .collect();

    // Elegimos qué apps guardar: las que más CPU, memoria y E/S usaron.
    let mut elegidas: HashSet<String> = HashSet::new();
    let mut elegir = |apps: &mut Vec<MinutoApp>, cuantas: usize, clave: fn(&MinutoApp) -> f64| {
        apps.sort_by(|x, y| clave(y).total_cmp(&clave(x)));
        for app in apps.iter().take(cuantas).filter(|x| clave(x) > 0.0) {
            elegidas.insert(app.nombre.clone());
        }
    };
    elegir(&mut apps, TOP_CPU, |x| x.cpu_prom);
    elegir(&mut apps, TOP_IO, |x| x.io_prom);
    elegir(&mut apps, TOP_MEMORIA, |x| x.mem_prom as f64);

    let (mut guardar, resto): (Vec<_>, Vec<_>) =
        apps.into_iter().partition(|x| elegidas.contains(&x.nombre));
    if !resto.is_empty() {
        guardar.push(MinutoApp {
            nombre: OTROS.to_string(),
            ruta: None,
            descripcion: Some(format!("{} aplicaciones más pequeñas", resto.len())),
            empresa: None,
            cpu_prom: resto.iter().map(|x| x.cpu_prom).sum(),
            cpu_max: resto.iter().map(|x| x.cpu_max).sum(),
            mem_prom: resto.iter().map(|x| x.mem_prom).sum(),
            mem_max: resto.iter().map(|x| x.mem_max).sum(),
            io_prom: resto.iter().map(|x| x.io_prom).sum(),
            procesos: resto.iter().map(|x| x.procesos).sum(),
        });
    }
    ResumenMinuto {
        sistema,
        apps: guardar,
    }
}

/// Guarda el resumen de un minuto (en una sola transacción).
pub fn guardar_minuto(conn: &mut Connection, r: &ResumenMinuto) -> Resultado<()> {
    let tx = conn.transaction()?;
    let s = &r.sistema;
    tx.execute(
        "INSERT OR REPLACE INTO minuto_sistema
         (ts, cpu_prom, cpu_max, mem_prom, mem_max, mem_total, red_rx, red_tx, muestras)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            s.ts,
            s.cpu_prom,
            s.cpu_max,
            s.mem_prom as i64,
            s.mem_max as i64,
            s.mem_total as i64,
            s.red_rx,
            s.red_tx,
            s.muestras
        ],
    )?;
    for app in &r.apps {
        // Inserta la app si es nueva (o completa sus datos) y devuelve su id.
        let id: i64 = tx.query_row(
            "INSERT INTO apps(nombre, ruta, descripcion, empresa) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(nombre) DO UPDATE SET
               ruta = COALESCE(excluded.ruta, ruta),
               descripcion = COALESCE(excluded.descripcion, descripcion),
               empresa = COALESCE(excluded.empresa, empresa)
             RETURNING id",
            params![app.nombre, app.ruta, app.descripcion, app.empresa],
            |f| f.get(0),
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO minuto_app
             (ts, app_id, cpu_prom, cpu_max, mem_prom, mem_max, io_prom, procesos)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                s.ts,
                id,
                app.cpu_prom,
                app.cpu_max,
                app.mem_prom as i64,
                app.mem_max as i64,
                app.io_prom,
                app.procesos
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Resultado de aplicar la retención.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Retencion {
    pub minutos_borrados: usize,
    pub bytes_usados: u64,
}

/// Borra lo más antiguo que `dias`, y si la base sigue ocupando más de
/// `tope_bytes`, borra por horas desde lo más viejo hasta quedar debajo
/// (nunca la última hora).
pub fn aplicar_retencion(
    conn: &Connection,
    ahora_ms: i64,
    dias: i64,
    tope_bytes: u64,
) -> Resultado<Retencion> {
    let limite = ahora_ms - dias * DIA_MS;
    let mut borrados = borrar_antes_de(conn, limite)?;
    conn.execute("DELETE FROM avisos WHERE ts < ?1", [limite])?;

    let ultima_hora = ahora_ms - 60 * MINUTO_MS;
    while base::bytes_usados(conn)? > tope_bytes {
        let mas_viejo: Option<i64> = conn
            .query_row("SELECT MIN(ts) FROM minuto_sistema", [], |f| f.get(0))
            .optional()?
            .flatten();
        let Some(viejo) = mas_viejo else { break };
        let corte = (minuto_de(viejo) + 60 * MINUTO_MS).min(ultima_hora);
        if corte <= viejo {
            break;
        }
        borrados += borrar_antes_de(conn, corte)?;
    }
    // Devuelve al sistema de archivos las páginas liberadas.
    conn.execute_batch("PRAGMA incremental_vacuum;")?;
    Ok(Retencion {
        minutos_borrados: borrados,
        bytes_usados: base::bytes_usados(conn)?,
    })
}

fn borrar_antes_de(conn: &Connection, ts: i64) -> Resultado<usize> {
    conn.execute("DELETE FROM minuto_app WHERE ts < ?1", [ts])?;
    Ok(conn.execute("DELETE FROM minuto_sistema WHERE ts < ?1", [ts])?)
}

/// Uso de una aplicación en un rango de tiempo.
#[derive(Debug, Clone, Serialize)]
pub struct UsoApp {
    pub nombre: String,
    pub ruta: Option<String>,
    pub descripcion: Option<String>,
    pub empresa: Option<String>,
    /// % de CPU promedio sobre **todo** el rango (contando como 0 los
    /// minutos en que no estuvo abierta).
    pub cpu_prom: f64,
    pub cpu_max: f64,
    /// Memoria promedio sobre todo el rango.
    pub mem_prom: u64,
    /// Memoria promedio solo mientras estuvo abierta.
    pub mem_prom_activa: u64,
    pub mem_max: u64,
    pub io_prom: f64,
    /// Minutos en que figuró entre las que más consumieron.
    pub minutos: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orden {
    Cpu,
    Memoria,
    Io,
}

impl Orden {
    pub fn desde_texto(t: &str) -> Orden {
        match t {
            "memoria" => Orden::Memoria,
            "io" => Orden::Io,
            _ => Orden::Cpu,
        }
    }
}

/// Minutos con datos en un rango.
pub fn minutos_con_datos(conn: &Connection, desde: i64, hasta: i64) -> Resultado<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM minuto_sistema WHERE ts >= ?1 AND ts < ?2",
        [desde, hasta],
        |f| f.get(0),
    )?)
}

/// Las aplicaciones que más consumieron en `[desde, hasta)`.
pub fn top_apps(
    conn: &Connection,
    desde: i64,
    hasta: i64,
    orden: Orden,
    limite: usize,
) -> Resultado<Vec<UsoApp>> {
    let total = minutos_con_datos(conn, desde, hasta)?.max(1) as f64;
    let columna = match orden {
        Orden::Cpu => "SUM(m.cpu_prom)",
        Orden::Memoria => "SUM(m.mem_prom)",
        Orden::Io => "SUM(m.io_prom)",
    };
    let sql = format!(
        "SELECT a.nombre, a.ruta, a.descripcion, a.empresa,
                SUM(m.cpu_prom), MAX(m.cpu_max), SUM(m.mem_prom), MAX(m.mem_max),
                SUM(m.io_prom), COUNT(*)
         FROM minuto_app m JOIN apps a ON a.id = m.app_id
         WHERE m.ts >= ?1 AND m.ts < ?2 AND a.nombre <> ?3
         GROUP BY m.app_id
         ORDER BY {columna} DESC
         LIMIT ?4"
    );
    let mut st = conn.prepare(&sql)?;
    let filas = st.query_map(params![desde, hasta, OTROS, limite as i64], |f| {
        let minutos: i64 = f.get(9)?;
        let mem_suma: f64 = f.get(6)?;
        Ok(UsoApp {
            nombre: f.get(0)?,
            ruta: f.get(1)?,
            descripcion: f.get(2)?,
            empresa: f.get(3)?,
            cpu_prom: f.get::<_, f64>(4)? / total,
            cpu_max: f.get(5)?,
            mem_prom: (mem_suma / total) as u64,
            mem_prom_activa: (mem_suma / minutos.max(1) as f64) as u64,
            mem_max: f.get::<_, i64>(7)? as u64,
            io_prom: f.get::<_, f64>(8)? / total,
            minutos: minutos as u32,
        })
    })?;
    Ok(filas.collect::<Result<Vec<_>, _>>()?)
}

/// Serie del sistema en un rango, con a lo sumo `max_puntos` puntos
/// (si hay más minutos, se agrupan en intervalos más largos).
pub fn linea_tiempo(
    conn: &Connection,
    desde: i64,
    hasta: i64,
    max_puntos: usize,
) -> Resultado<Vec<MinutoSistema>> {
    let rango = (hasta - desde).max(MINUTO_MS);
    let minutos = rango / MINUTO_MS;
    let paso_min = (minutos + max_puntos.max(1) as i64 - 1) / max_puntos.max(1) as i64;
    let paso = paso_min.max(1) * MINUTO_MS;
    let mut st = conn.prepare(
        "SELECT ?1 + ((ts - ?1) / ?3) * ?3 AS grupo,
                AVG(cpu_prom), MAX(cpu_max), AVG(mem_prom), MAX(mem_max), MAX(mem_total),
                AVG(red_rx), AVG(red_tx), SUM(muestras)
         FROM minuto_sistema WHERE ts >= ?1 AND ts < ?2
         GROUP BY grupo ORDER BY grupo",
    )?;
    let filas = st.query_map(params![desde, hasta, paso], |f| {
        Ok(MinutoSistema {
            ts: f.get(0)?,
            cpu_prom: f.get(1)?,
            cpu_max: f.get(2)?,
            mem_prom: f.get::<_, f64>(3)? as u64,
            mem_max: f.get::<_, i64>(4)? as u64,
            mem_total: f.get::<_, i64>(5)? as u64,
            red_rx: f.get(6)?,
            red_tx: f.get(7)?,
            muestras: f.get::<_, i64>(8)? as u32,
        })
    })?;
    Ok(filas.collect::<Result<Vec<_>, _>>()?)
}

/// Qué estaba pasando en un momento: el minuto guardado más cercano
/// (hasta 10 minutos de distancia) y sus aplicaciones.
#[derive(Debug, Clone, Serialize)]
pub struct Momento {
    pub sistema: MinutoSistema,
    pub apps: Vec<MinutoApp>,
}

pub fn momento(conn: &Connection, ts: i64) -> Resultado<Option<Momento>> {
    let sistema = conn
        .query_row(
            "SELECT ts, cpu_prom, cpu_max, mem_prom, mem_max, mem_total, red_rx, red_tx, muestras
             FROM minuto_sistema WHERE ts BETWEEN ?1 - 600000 AND ?1 + 600000
             ORDER BY ABS(ts - ?1) LIMIT 1",
            [minuto_de(ts)],
            |f| {
                Ok(MinutoSistema {
                    ts: f.get(0)?,
                    cpu_prom: f.get(1)?,
                    cpu_max: f.get(2)?,
                    mem_prom: f.get::<_, i64>(3)? as u64,
                    mem_max: f.get::<_, i64>(4)? as u64,
                    mem_total: f.get::<_, i64>(5)? as u64,
                    red_rx: f.get(6)?,
                    red_tx: f.get(7)?,
                    muestras: f.get::<_, i64>(8)? as u32,
                })
            },
        )
        .optional()?;
    let Some(sistema) = sistema else {
        return Ok(None);
    };
    let mut st = conn.prepare(
        "SELECT a.nombre, a.ruta, a.descripcion, a.empresa,
                m.cpu_prom, m.cpu_max, m.mem_prom, m.mem_max, m.io_prom, m.procesos
         FROM minuto_app m JOIN apps a ON a.id = m.app_id
         WHERE m.ts = ?1 ORDER BY m.cpu_prom DESC",
    )?;
    let apps = st
        .query_map([sistema.ts], |f| {
            Ok(MinutoApp {
                nombre: f.get(0)?,
                ruta: f.get(1)?,
                descripcion: f.get(2)?,
                empresa: f.get(3)?,
                cpu_prom: f.get(4)?,
                cpu_max: f.get(5)?,
                mem_prom: f.get::<_, i64>(6)? as u64,
                mem_max: f.get::<_, i64>(7)? as u64,
                io_prom: f.get(8)?,
                procesos: f.get::<_, i64>(9)? as u32,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Momento { sistema, apps }))
}

#[cfg(test)]
pub(crate) mod pruebas {
    use super::*;
    use crate::colector::{Proceso, Sistema};

    /// Proceso falso para las pruebas.
    pub fn proceso(pid: u32, nombre: &str, cpu: f64, mem_mb: u64) -> Proceso {
        Proceso {
            pid,
            ppid: 1,
            nombre: nombre.to_string(),
            ruta: Some(format!("C:\\prueba\\{nombre}")),
            cpu,
            memoria: mem_mb * 1_048_576,
            memoria_fisica: mem_mb * 1_048_576,
            memoria_comprometida: mem_mb * 1_048_576,
            io_lectura_bps: 0.0,
            io_escritura_bps: 0.0,
            io_lectura_total: 0,
            io_escritura_total: 0,
            inicio_ms: Some(1000 + i64::from(pid)),
            descripcion: Some(format!("Descripción de {nombre}")),
            empresa: Some("Empresa de prueba".into()),
            hilos: 1,
            manijas: 10,
            acceso_limitado: false,
        }
    }

    /// Muestra falsa con hora `ts` y la CPU del sistema = suma de procesos.
    pub fn muestra(ts: i64, procesos: Vec<Proceso>) -> Muestra {
        let cpu = procesos.iter().map(|p| p.cpu).sum::<f64>().min(100.0);
        let mem = procesos.iter().map(|p| p.memoria).sum();
        Muestra {
            ts,
            sistema: Sistema {
                cpu,
                mem_total: 16 << 30,
                mem_usada: mem,
                n_procesos: procesos.len(),
                n_cpu: 4,
                ..Default::default()
            },
            procesos,
            unidades: vec![],
            adaptadores: vec![],
        }
    }

    /// Reloj simulado: alimenta `minutos` minutos de muestras cada 2 s,
    /// empezando en `inicio`, y guarda cada minuto cerrado.
    pub fn simular(
        conn: &mut Connection,
        h: &mut Historial,
        inicio: i64,
        minutos: i64,
        f: impl Fn(i64) -> Vec<Proceso>,
    ) {
        let mut ts = inicio;
        while ts < inicio + minutos * MINUTO_MS {
            if let Some(r) = h.registrar(&muestra(ts, f(ts))) {
                guardar_minuto(conn, &r).unwrap();
            }
            ts += 2000;
        }
    }

    const T0: i64 = 1_790_000_000_000; // un martes de 2026, en ms

    #[test]
    fn agrupa_por_app_y_cierra_minutos() {
        let mut h = Historial::new(300);
        // 30 muestras en el primer minuto: dos chrome y un notepad.
        let mut cerrado = None;
        for i in 0..30 {
            let ts = minuto_de(T0) + i * 2000;
            cerrado = h.registrar(&muestra(
                ts,
                vec![
                    proceso(1, "chrome.exe", 10.0, 100),
                    proceso(2, "Chrome.exe", 5.0, 50),
                    proceso(3, "notepad.exe", 1.0, 10),
                ],
            ));
            assert!(cerrado.is_none());
        }
        assert_eq!(h.proceso(1, 1001).len(), 30);
        // La primera muestra del minuto siguiente cierra el anterior.
        cerrado = cerrado.or(h.registrar(&muestra(minuto_de(T0) + MINUTO_MS, vec![])));
        let r = cerrado.unwrap();
        assert_eq!(r.sistema.ts, minuto_de(T0));
        assert_eq!(r.sistema.muestras, 30);
        assert!((r.sistema.cpu_prom - 16.0).abs() < 1e-9);
        let chrome = r.apps.iter().find(|a| a.nombre == "chrome.exe").unwrap();
        assert!((chrome.cpu_prom - 15.0).abs() < 1e-9);
        assert_eq!(chrome.mem_prom, 150 * 1_048_576);
        assert_eq!(chrome.procesos, 2);
        assert_eq!(chrome.empresa.as_deref(), Some("Empresa de prueba"));
        assert_eq!(h.vivo().count(), 31);
        // El proceso 1 ya no está en la última muestra: se olvida.
        assert!(h.proceso(1, 1001).is_empty());
    }

    #[test]
    fn vivo_respeta_capacidad() {
        let mut h = Historial::new(5);
        for i in 0..20 {
            h.registrar(&muestra(T0 + i * 2000, vec![proceso(1, "a.exe", 1.0, 1)]));
        }
        assert_eq!(h.vivo().count(), 5);
        assert_eq!(h.vivo().next().unwrap().ts, T0 + 15 * 2000);
    }

    #[test]
    fn solo_guarda_las_que_mas_consumen_y_junta_el_resto() {
        let mut h = Historial::new(10);
        let procesos: Vec<_> = (0..60)
            .map(|i| proceso(i, &format!("app{i}.exe"), f64::from(i) / 10.0, u64::from(i)))
            .collect();
        h.registrar(&muestra(minuto_de(T0), procesos));
        let r = h.cerrar().unwrap();
        // 12 por CPU y 12 por memoria coinciden (las mismas apps grandes),
        // más la fila '(otros)'.
        assert!(r.apps.len() <= TOP_CPU + TOP_MEMORIA + TOP_IO + 1);
        let otros = r.apps.iter().find(|a| a.nombre == OTROS).unwrap();
        // Lo guardado (incluida '(otros)') suma lo mismo que todos los
        // procesos: 0,0 + 0,1 + ... + 5,9 = 177.
        let total: f64 = r.apps.iter().map(|a| a.cpu_prom).sum();
        assert!((total - 177.0).abs() < 1e-6, "{total}");
        assert!(otros.cpu_prom > 0.0);
        assert!(r.apps.iter().any(|a| a.nombre == "app59.exe"));
    }

    #[test]
    fn consultas_en_un_rango_con_reloj_simulado() {
        let mut conn = base::en_memoria().unwrap();
        let mut h = Historial::new(300);
        let inicio = minuto_de(T0);
        // 2 horas: la primera hora "juego.exe" usa 50 % de CPU; la segunda,
        // "editor.exe" usa 3 GB de memoria.
        simular(&mut conn, &mut h, inicio, 120, |ts| {
            if ts < inicio + 60 * MINUTO_MS {
                vec![
                    proceso(1, "juego.exe", 50.0, 500),
                    proceso(2, "editor.exe", 1.0, 100),
                ]
            } else {
                vec![proceso(2, "editor.exe", 2.0, 3000)]
            }
        });
        assert_eq!(
            minutos_con_datos(&conn, inicio, inicio + 120 * MINUTO_MS).unwrap(),
            119 // el último minuto sigue abierto en memoria
        );

        // CPU en las 2 horas: gana juego.exe con ~25 % de promedio.
        let top = top_apps(&conn, inicio, inicio + 120 * MINUTO_MS, Orden::Cpu, 5).unwrap();
        assert_eq!(top[0].nombre, "juego.exe");
        assert!((top[0].cpu_prom - 50.0 * 60.0 / 119.0).abs() < 0.01);
        assert_eq!(top[0].minutos, 60);
        assert_eq!(
            top[0].descripcion.as_deref(),
            Some("Descripción de juego.exe")
        );

        // Memoria en la segunda hora: gana editor.exe.
        let top = top_apps(
            &conn,
            inicio + 60 * MINUTO_MS,
            inicio + 120 * MINUTO_MS,
            Orden::Memoria,
            5,
        )
        .unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].nombre, "editor.exe");
        assert_eq!(top[0].mem_prom_activa, 3000 * 1_048_576);

        // Momento a los 30 minutos: juego.exe arriba.
        let m = momento(&conn, inicio + 30 * MINUTO_MS + 12_345)
            .unwrap()
            .unwrap();
        assert_eq!(m.sistema.ts, inicio + 30 * MINUTO_MS);
        assert_eq!(m.apps[0].nombre, "juego.exe");
        // Momento muy lejos de cualquier dato: nada.
        assert!(momento(&conn, inicio - DIA_MS).unwrap().is_none());

        // Línea de tiempo con máximo 12 puntos: 119 minutos en grupos de 10.
        let linea = linea_tiempo(&conn, inicio, inicio + 120 * MINUTO_MS, 12).unwrap();
        assert_eq!(linea.len(), 12);
        assert!((linea[0].cpu_prom - 51.0).abs() < 1e-6);
        assert!((linea[11].cpu_prom - 2.0).abs() < 1e-6);
    }

    #[test]
    fn retencion_por_dias() {
        let mut conn = base::en_memoria().unwrap();
        let mut h = Historial::new(10);
        let inicio = minuto_de(T0);
        // Un minuto hace 40 días, otro hace 10 días.
        for dia in [40, 10] {
            let ts = inicio - dia * DIA_MS;
            simular(&mut conn, &mut h, ts, 2, |_| {
                vec![proceso(1, "a.exe", 1.0, 1)]
            });
        }
        let antes: i64 = conn
            .query_row("SELECT COUNT(*) FROM minuto_sistema", [], |f| f.get(0))
            .unwrap();
        assert_eq!(antes, 3);
        let r = aplicar_retencion(&conn, inicio, 30, u64::MAX).unwrap();
        assert_eq!(r.minutos_borrados, 2); // los del día -40 (2 minutos)
        let quedan: i64 = conn
            .query_row("SELECT MIN(ts) FROM minuto_sistema", [], |f| f.get(0))
            .unwrap();
        assert!(quedan > inicio - 30 * DIA_MS);
    }

    #[test]
    fn retencion_por_tamano() {
        let mut conn = base::en_memoria().unwrap();
        let mut h = Historial::new(10);
        let inicio = minuto_de(T0);
        // 6 horas con 40 apps distintas por minuto.
        simular(&mut conn, &mut h, inicio, 360, |ts| {
            (0..40)
                .map(|i| proceso(i, &format!("app{i}_{}.exe", ts / 3_600_000), 1.0, 10))
                .collect()
        });
        let ahora = inicio + 360 * MINUTO_MS;
        let usado = base::bytes_usados(&conn).unwrap();
        let tope = usado / 2;
        let r = aplicar_retencion(&conn, ahora, 30, tope).unwrap();
        assert!(r.bytes_usados <= tope, "{} > {}", r.bytes_usados, tope);
        assert!(r.minutos_borrados > 0);
        // Lo más nuevo se conserva.
        let max: i64 = conn
            .query_row("SELECT MAX(ts) FROM minuto_sistema", [], |f| f.get(0))
            .unwrap();
        assert_eq!(max, ahora - 2 * MINUTO_MS);
        // Con un tope imposible, nunca se borra la última hora.
        let r = aplicar_retencion(&conn, ahora, 30, 1).unwrap();
        let min: i64 = conn
            .query_row("SELECT MIN(ts) FROM minuto_sistema", [], |f| f.get(0))
            .unwrap();
        assert!(min >= ahora - 60 * MINUTO_MS, "{r:?}");
    }
}

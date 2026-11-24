//! Avisos: reglas configurables, el motor que las evalúa en cada muestra,
//! y el registro de avisos en la tabla `avisos`.
//!
//! El motor ([`Motor`]) no lee el reloj: usa la hora de cada muestra. Así
//! las pruebas simulan "5 minutos de memoria alta" con muestras falsas.

use crate::colector::Muestra;
use crate::error::Resultado;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------
// Reglas
// ---------------------------------------------------------------------

/// Reglas de avisos, editables desde la interfaz.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Reglas {
    /// Memoria del sistema por encima de `memoria_pct` durante `memoria_min`.
    pub memoria_activa: bool,
    pub memoria_pct: f64,
    pub memoria_min: u32,
    /// Un proceso por encima de `cpu_pct` (% del equipo) durante `cpu_min`.
    pub cpu_activa: bool,
    pub cpu_pct: f64,
    pub cpu_min: u32,
    /// Una unidad con menos de `disco_pct` % libre.
    pub disco_activa: bool,
    pub disco_pct: f64,
    /// Entradas de inicio nuevas o quitadas.
    pub inicio_activa: bool,
    /// Puertos TCP en escucha nuevos.
    pub puerto_activa: bool,
    /// Avisar solo de puertos accesibles desde otros equipos (no de los que
    /// escuchan en 127.0.0.1, típicos de herramientas de desarrollo).
    pub puerto_solo_red: bool,
    /// Mostrar además una notificación de Windows.
    pub notificar_windows: bool,
}

impl Default for Reglas {
    fn default() -> Self {
        Self {
            memoria_activa: true,
            memoria_pct: 90.0,
            memoria_min: 5,
            cpu_activa: true,
            cpu_pct: 50.0,
            cpu_min: 5,
            disco_activa: true,
            disco_pct: 10.0,
            inicio_activa: true,
            puerto_activa: true,
            puerto_solo_red: true,
            notificar_windows: false,
        }
    }
}

impl Reglas {
    /// Lleva cada valor a un rango razonable (lo que llega de la interfaz
    /// no se acepta a ciegas).
    pub fn corregidas(mut self) -> Reglas {
        let pct = |v: f64, min: f64| {
            if v.is_finite() {
                v.clamp(min, 100.0)
            } else {
                min
            }
        };
        self.memoria_pct = pct(self.memoria_pct, 10.0);
        self.cpu_pct = pct(self.cpu_pct, 1.0);
        self.disco_pct = pct(self.disco_pct, 0.1).min(99.0);
        self.memoria_min = self.memoria_min.clamp(1, 1440);
        self.cpu_min = self.cpu_min.clamp(1, 1440);
        self
    }

    /// ¿Hay que avisar de este tipo de cambio? (inicio y puertos).
    pub fn permite(&self, tipo: &str) -> bool {
        match tipo {
            "inicio_nuevo" | "inicio_quitado" => self.inicio_activa,
            "puerto_nuevo" => self.puerto_activa,
            "puerto_nuevo_local" => self.puerto_activa && !self.puerto_solo_red,
            _ => true,
        }
    }
}

pub const CLAVE_REGLAS: &str = "reglas";

pub fn leer_reglas(conn: &Connection) -> Resultado<Reglas> {
    Ok(crate::base::leer_config::<Reglas>(conn, CLAVE_REGLAS)?
        .unwrap_or_default()
        .corregidas())
}

pub fn guardar_reglas(conn: &Connection, r: &Reglas) -> Resultado<()> {
    crate::base::guardar_config(conn, CLAVE_REGLAS, r)
}

// ---------------------------------------------------------------------
// Motor
// ---------------------------------------------------------------------

/// Un aviso que el motor quiere registrar.
#[derive(Debug, Clone, PartialEq)]
pub struct Nuevo {
    pub tipo: &'static str,
    pub titulo: String,
    pub detalle: String,
}

/// No repetir el mismo aviso (misma regla y mismo sujeto) antes de esto.
const ESPERA_REPETIR_MS: i64 = 60 * 60_000;
const ESPERA_REPETIR_DISCO_MS: i64 = 6 * 60 * 60_000;

/// Estado del motor entre muestras.
#[derive(Default)]
pub struct Motor {
    /// Desde cuándo la memoria está por encima del umbral.
    memoria_desde: Option<i64>,
    /// Por proceso (PID, inicio): desde cuándo su CPU está por encima.
    cpu_desde: HashMap<(u32, i64), i64>,
    /// Última vez que se avisó de cada cosa.
    ultimo: HashMap<String, i64>,
}

impl Motor {
    /// `true` si no se avisó de `clave` en los últimos `espera` ms (y lo
    /// anota como avisado ahora).
    fn puede(&mut self, clave: String, ts: i64, espera: i64) -> bool {
        match self.ultimo.get(&clave) {
            Some(&t) if ts - t < espera => false,
            _ => {
                self.ultimo.insert(clave, ts);
                true
            }
        }
    }

    /// Evalúa las reglas contra una muestra y devuelve los avisos nuevos.
    pub fn evaluar(&mut self, r: &Reglas, m: &Muestra) -> Vec<Nuevo> {
        let ts = m.ts;
        let mut nuevos = Vec::new();

        // --- Memoria del sistema alta y sostenida ---
        let total = m.sistema.mem_total.max(1) as f64;
        let mem_pct = m.sistema.mem_usada as f64 / total * 100.0;
        if r.memoria_activa && mem_pct >= r.memoria_pct {
            let desde = *self.memoria_desde.get_or_insert(ts);
            let min = i64::from(r.memoria_min) * 60_000;
            if ts - desde >= min && self.puede("memoria".into(), ts, ESPERA_REPETIR_MS) {
                // El proceso que más memoria usa ayuda a entender el aviso.
                let mayor = m.procesos.iter().max_by_key(|p| p.memoria);
                nuevos.push(Nuevo {
                    tipo: "memoria_alta",
                    titulo: format!(
                        "Memoria alta: {mem_pct:.0} % durante {} min",
                        (ts - desde) / 60_000
                    ),
                    detalle: match mayor {
                        Some(p) => format!(
                            "El proceso que más memoria usa es {} ({} MB).",
                            p.nombre,
                            p.memoria / 1_048_576
                        ),
                        None => String::new(),
                    },
                });
            }
        } else {
            self.memoria_desde = None;
        }

        // --- Un proceso con CPU alta sostenida ---
        if r.cpu_activa {
            let min = i64::from(r.cpu_min) * 60_000;
            let mut siguen = HashMap::new();
            for p in &m.procesos {
                if p.cpu < r.cpu_pct {
                    continue;
                }
                let clave = (p.pid, p.inicio_ms.unwrap_or(0));
                let desde = self.cpu_desde.get(&clave).copied().unwrap_or(ts);
                siguen.insert(clave, desde);
                let id = format!("cpu|{}|{}", clave.0, clave.1);
                if ts - desde >= min && self.puede(id, ts, ESPERA_REPETIR_MS) {
                    nuevos.push(Nuevo {
                        tipo: "cpu_alta",
                        titulo: format!(
                            "{} usa {:.0} % de CPU desde hace {} min",
                            p.nombre,
                            p.cpu,
                            (ts - desde) / 60_000
                        ),
                        detalle: format!(
                            "PID {}. {}",
                            p.pid,
                            p.ruta.as_deref().unwrap_or("Ruta no disponible")
                        ),
                    });
                }
            }
            // Los que bajaron del umbral (o terminaron) empiezan de cero.
            self.cpu_desde = siguen;
        } else {
            self.cpu_desde.clear();
        }

        // --- Poco espacio libre en una unidad ---
        if r.disco_activa {
            for u in &m.unidades {
                if u.total == 0 {
                    continue;
                }
                let libre = u.libre as f64 / u.total as f64 * 100.0;
                if libre < r.disco_pct
                    && self.puede(format!("disco|{}", u.raiz), ts, ESPERA_REPETIR_DISCO_MS)
                {
                    nuevos.push(Nuevo {
                        tipo: "disco_poco",
                        titulo: format!("Poco espacio en {}: {libre:.1} % libre", u.raiz),
                        detalle: format!(
                            "Quedan {:.1} GB de {:.1} GB. La pantalla Disco ayuda a ver qué ocupa más.",
                            u.libre as f64 / 1_073_741_824.0,
                            u.total as f64 / 1_073_741_824.0
                        ),
                    });
                }
            }
        }
        nuevos
    }
}

// ---------------------------------------------------------------------
// Registro en la base
// ---------------------------------------------------------------------

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
    use crate::colector::Unidad;
    use crate::historial::pruebas::{muestra, proceso};

    const T0: i64 = 1_790_000_000_000;
    const MB: u64 = 1_048_576;

    /// Muestra con `mem_mb` MB usados de 1000 MB y los procesos dados.
    fn con_memoria(ts: i64, mem_mb: u64, procesos: Vec<crate::colector::Proceso>) -> Muestra {
        let mut m = muestra(ts, procesos);
        m.sistema.mem_total = 1000 * MB;
        m.sistema.mem_usada = mem_mb * MB;
        m
    }

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

    #[test]
    fn memoria_alta_avisa_solo_si_se_sostiene() {
        let r = Reglas::default(); // 90 % durante 5 min
        let mut motor = Motor::default();
        // 4 minutos al 95 %: no avisa todavía.
        for s in 0..=120 {
            let a = motor.evaluar(&r, &con_memoria(T0 + s * 2000, 950, vec![]));
            assert!(a.is_empty(), "avisó antes de tiempo en {s}");
        }
        // Baja al 50 % un momento: se reinicia la cuenta.
        assert!(
            motor
                .evaluar(&r, &con_memoria(T0 + 242_000, 500, vec![]))
                .is_empty()
        );
        let base = T0 + 244_000;
        let mut avisos = Vec::new();
        for s in 0..=160 {
            let m = con_memoria(
                base + s * 2000,
                950,
                vec![proceso(7, "grande.exe", 0.0, 800)],
            );
            avisos.extend(motor.evaluar(&r, &m));
        }
        // 5 min = 150 muestras después del reinicio: exactamente un aviso.
        assert_eq!(avisos.len(), 1, "{avisos:?}");
        assert_eq!(avisos[0].tipo, "memoria_alta");
        assert!(avisos[0].titulo.contains("95 %"));
        assert!(avisos[0].detalle.contains("grande.exe"));
    }

    #[test]
    fn cpu_alta_sostenida_por_proceso_y_sin_repetir() {
        let r = Reglas {
            cpu_min: 2,
            ..Reglas::default()
        };
        let mut motor = Motor::default();
        let mut avisos = Vec::new();
        // 30 minutos: ocupado.exe al 80 % y tranquilo.exe al 5 %.
        for s in 0..900 {
            let procesos = vec![
                proceso(1, "ocupado.exe", 80.0, 1),
                proceso(2, "tranquilo.exe", 5.0, 1),
            ];
            avisos.extend(motor.evaluar(&r, &con_memoria(T0 + s * 2000, 100, procesos)));
        }
        // Un solo aviso (la espera para repetir es de 1 hora).
        assert_eq!(avisos.len(), 1, "{avisos:?}");
        assert!(avisos[0].titulo.starts_with("ocupado.exe usa 80 %"));

        // Regla desactivada: nada.
        let mut motor = Motor::default();
        let r = Reglas {
            cpu_activa: false,
            ..r
        };
        for s in 0..200 {
            let m = con_memoria(T0 + s * 2000, 100, vec![proceso(1, "ocupado.exe", 80.0, 1)]);
            assert!(motor.evaluar(&r, &m).is_empty());
        }
    }

    #[test]
    fn poco_espacio_en_disco() {
        let r = Reglas::default(); // menos de 10 % libre
        let mut motor = Motor::default();
        let mut m = con_memoria(T0, 100, vec![]);
        let unidad = |raiz: &str, libre| Unidad {
            raiz: raiz.into(),
            etiqueta: String::new(),
            tipo: "fijo",
            total: 1000,
            libre,
        };
        m.unidades = vec![unidad("C:", 50), unidad("D:", 500)];
        let a = motor.evaluar(&r, &m);
        assert_eq!(a.len(), 1);
        assert!(a[0].titulo.contains("C:"));
        assert!(a[0].titulo.contains("5.0 % libre"));
        // Una hora después no repite (espera de 6 h); a las 7 h sí.
        m.ts = T0 + 3_600_000;
        assert!(motor.evaluar(&r, &m).is_empty());
        m.ts = T0 + 7 * 3_600_000;
        assert_eq!(motor.evaluar(&r, &m).len(), 1);
    }

    #[test]
    fn reglas_se_corrigen_y_se_guardan() {
        let c = crate::base::en_memoria().unwrap();
        assert_eq!(leer_reglas(&c).unwrap(), Reglas::default());
        let r = Reglas {
            memoria_pct: 500.0,
            cpu_min: 0,
            disco_pct: f64::NAN,
            ..Reglas::default()
        }
        .corregidas();
        assert_eq!(r.memoria_pct, 100.0);
        assert_eq!(r.cpu_min, 1);
        assert_eq!(r.disco_pct, 0.1);
        guardar_reglas(&c, &r).unwrap();
        assert_eq!(leer_reglas(&c).unwrap(), r);
        let sin_puertos = Reglas {
            puerto_activa: false,
            ..r.clone()
        };
        assert!(!sin_puertos.permite("puerto_nuevo"));
        assert!(r.permite("inicio_nuevo"));
        assert!(r.permite("puerto_nuevo"));
        assert!(!r.permite("puerto_nuevo_local")); // solo_red activa por omisión
    }
}

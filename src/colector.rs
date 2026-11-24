//! Colector: toma "muestras" del sistema y las convierte en tasas.
//!
//! Windows entrega casi todo como **contadores acumulados**: tiempo total de
//! CPU que usó un proceso desde que nació, bytes totales recibidos por un
//! adaptador, etc. Un porcentaje o una velocidad solo existen al comparar dos
//! lecturas separadas por un intervalo conocido. Por eso el colector guarda
//! la lectura anterior de cada cosa y calcula la diferencia.

use crate::win;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Un proceso en una muestra, listo para mostrarse.
#[derive(Debug, Clone, Serialize)]
pub struct Proceso {
    pub pid: u32,
    pub ppid: u32,
    pub nombre: String,
    pub ruta: Option<String>,
    /// % de CPU del equipo completo (0–100), como el Administrador de tareas.
    pub cpu: f64,
    /// Memoria física privada (*private working set*), en bytes. Es la
    /// columna «Memoria» del Administrador de tareas.
    pub memoria: u64,
    /// Memoria física total en uso, incluida la compartida con otros.
    pub memoria_fisica: u64,
    /// Memoria privada comprometida (RAM + archivo de paginación).
    pub memoria_comprometida: u64,
    pub io_lectura_bps: f64,
    pub io_escritura_bps: f64,
    pub io_lectura_total: u64,
    pub io_escritura_total: u64,
    /// Hora de inicio, ms desde 1970 (UTC).
    pub inicio_ms: Option<i64>,
    pub descripcion: Option<String>,
    pub empresa: Option<String>,
    pub hilos: u32,
    pub manijas: u32,
    /// `true` si Windows no dejó leer la ruta del ejecutable (típico de
    /// procesos protegidos del sistema cuando Vigía corre sin administrador).
    pub acceso_limitado: bool,
}

/// Totales del sistema en una muestra.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Sistema {
    pub cpu: f64,
    pub mem_total: u64,
    pub mem_usada: u64,
    pub red_rx_bps: f64,
    pub red_tx_bps: f64,
    pub n_procesos: usize,
    pub n_cpu: u32,
    pub ms_desde_arranque: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Unidad {
    pub raiz: String,
    pub etiqueta: String,
    pub tipo: &'static str,
    pub total: u64,
    pub libre: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Adaptador {
    pub nombre: String,
    pub descripcion: String,
    pub conectado: bool,
    pub recibidos: u64,
    pub enviados: u64,
    pub rx_bps: f64,
    pub tx_bps: f64,
}

/// Una muestra completa del sistema en un instante.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Muestra {
    /// Momento de la muestra, ms desde 1970 (UTC).
    pub ts: i64,
    pub sistema: Sistema,
    pub procesos: Vec<Proceso>,
    pub unidades: Vec<Unidad>,
    pub adaptadores: Vec<Adaptador>,
}

/// Lectura anterior de un proceso, para calcular diferencias.
#[derive(Debug, Clone, Copy)]
struct Anterior {
    cpu_100ns: u64,
    io_lectura: u64,
    io_escritura: u64,
}

/// Cada cuánto se vuelve a consultar el espacio en disco (cambia lento y
/// consultar una unidad USB puede despertarla).
const INTERVALO_UNIDADES: Duration = Duration::from_secs(30);

/// Datos de un proceso que no cambian mientras vive.
#[derive(Debug, Clone)]
struct Estatico {
    ruta: Option<String>,
    descripcion: Option<String>,
    empresa: Option<String>,
}

/// Guarda el estado entre muestras.
pub struct Colector {
    lector: win::proceso::LectorProcesos,
    estaticos: HashMap<(u32, i64), Estatico>,
    n_cpu: u32,
    instante_anterior: Option<Instant>,
    cpu_anterior: Option<win::sistema::TiemposCpu>,
    /// Clave: (PID, hora de inicio). Windows reutiliza los PID, así que el
    /// PID solo no basta para saber si es "el mismo" proceso.
    procesos_anteriores: HashMap<(u32, i64), Anterior>,
    red_anterior: HashMap<u64, (u64, u64)>,
    /// Descripción y empresa por ruta de ejecutable. Leerlas implica abrir el
    /// archivo, así que se hace una sola vez por ruta.
    cache_version: HashMap<String, win::version::InfoVersion>,
    unidades: Vec<Unidad>,
    instante_unidades: Option<Instant>,
}

impl Default for Colector {
    fn default() -> Self {
        Self::new()
    }
}

impl Colector {
    pub fn new() -> Self {
        Self {
            lector: win::proceso::LectorProcesos::default(),
            estaticos: HashMap::new(),
            n_cpu: win::sistema::procesadores(),
            instante_anterior: None,
            cpu_anterior: None,
            procesos_anteriores: HashMap::new(),
            red_anterior: HashMap::new(),
            cache_version: HashMap::new(),
            unidades: Vec::new(),
            instante_unidades: None,
        }
    }

    /// Toma una muestra. `ts` es la hora a registrar (ms Unix); los
    /// intervalos se miden con un reloj monótono para no confundirse si
    /// cambia la hora del sistema.
    ///
    /// La primera muestra tiene tasas en cero (no hay con qué comparar).
    pub fn muestrear(&mut self, ts: i64) -> win::Resultado<Muestra> {
        let ahora = Instant::now();
        let segundos = self
            .instante_anterior
            .map(|t| ahora.duration_since(t).as_secs_f64())
            .filter(|s| *s > 0.0);
        // Ojo: `instante_anterior` se actualiza al final, solo si la muestra
        // salió bien. Si se actualizara aquí y luego fallara la lista de
        // procesos, la próxima muestra dividiría 4 s de CPU por 2 s.

        // --- CPU total ---
        let cpu_ahora = win::sistema::tiempos_cpu()?;
        let cpu = self
            .cpu_anterior
            .map(|a| cpu_sistema(&a, &cpu_ahora))
            .unwrap_or(0.0);
        self.cpu_anterior = Some(cpu_ahora);

        // --- Memoria ---
        let mem = win::sistema::memoria()?;

        // --- Procesos ---
        let procesos = self.muestrear_procesos(segundos)?;

        // --- Red ---
        let adaptadores = self.muestrear_red(segundos);
        let red_rx_bps = adaptadores.iter().map(|a| a.rx_bps).sum();
        let red_tx_bps = adaptadores.iter().map(|a| a.tx_bps).sum();

        // --- Unidades (cada 30 s) ---
        if self
            .instante_unidades
            .is_none_or(|t| ahora.duration_since(t) >= INTERVALO_UNIDADES)
        {
            self.instante_unidades = Some(ahora);
            if let Ok(lista) = win::sistema::unidades() {
                self.unidades = lista
                    .into_iter()
                    .map(|u| Unidad {
                        raiz: u.raiz,
                        etiqueta: u.etiqueta,
                        tipo: u.tipo,
                        total: u.total,
                        libre: u.libre,
                    })
                    .collect();
            }
        }

        self.instante_anterior = Some(ahora);
        Ok(Muestra {
            ts,
            sistema: Sistema {
                cpu,
                mem_total: mem.total,
                mem_usada: mem.total.saturating_sub(mem.disponible),
                red_rx_bps,
                red_tx_bps,
                n_procesos: procesos.len(),
                n_cpu: self.n_cpu,
                ms_desde_arranque: win::sistema::ms_desde_arranque(),
            },
            procesos,
            unidades: self.unidades.clone(),
            adaptadores,
        })
    }

    fn muestrear_procesos(&mut self, segundos: Option<f64>) -> win::Resultado<Vec<Proceso>> {
        let lista = self.lector.leer()?;
        let mut procesos = Vec::with_capacity(lista.len());
        let mut vistos = HashSet::with_capacity(lista.len());
        for e in lista {
            if e.pid == 0 {
                // El "proceso inactivo del sistema" no es un programa: su
                // "CPU" es justamente la CPU libre. Se omite.
                continue;
            }
            let clave = (e.pid, e.inicio_ms.unwrap_or(0));
            vistos.insert(clave);

            let actual = Anterior {
                cpu_100ns: e.cpu_100ns,
                io_lectura: e.io_lectura,
                io_escritura: e.io_escritura,
            };
            let (cpu, io_l, io_e) = match (segundos, self.procesos_anteriores.get(&clave)) {
                (Some(s), Some(a)) => (
                    cpu_proceso(a.cpu_100ns, actual.cpu_100ns, s, self.n_cpu),
                    tasa(a.io_lectura, actual.io_lectura, s),
                    tasa(a.io_escritura, actual.io_escritura, s),
                ),
                _ => (0.0, 0.0, 0.0),
            };
            self.procesos_anteriores.insert(clave, actual);

            // Ruta, descripción y empresa no cambian durante la vida de un
            // proceso: se leen una sola vez y quedan en caché.
            let estatico = match self.estaticos.get(&clave) {
                Some(x) => x.clone(),
                None => {
                    let ruta = win::proceso::ruta(e.pid);
                    let version = match &ruta {
                        Some(r) => self.version_de(r),
                        None => win::version::InfoVersion::default(),
                    };
                    let x = Estatico {
                        ruta,
                        descripcion: version.descripcion,
                        empresa: version.empresa,
                    };
                    self.estaticos.insert(clave, x.clone());
                    x
                }
            };
            procesos.push(Proceso {
                pid: e.pid,
                ppid: e.ppid,
                nombre: e.nombre,
                acceso_limitado: estatico.ruta.is_none(),
                ruta: estatico.ruta,
                cpu,
                memoria: e.memoria_privada_fisica,
                memoria_fisica: e.memoria_fisica,
                memoria_comprometida: e.memoria_comprometida,
                io_lectura_bps: io_l,
                io_escritura_bps: io_e,
                io_lectura_total: actual.io_lectura,
                io_escritura_total: actual.io_escritura,
                inicio_ms: e.inicio_ms,
                descripcion: estatico.descripcion,
                empresa: estatico.empresa,
                hilos: e.hilos,
                manijas: e.manijas,
            });
        }
        // Olvidar procesos que ya terminaron, para no crecer sin límite.
        self.procesos_anteriores.retain(|k, _| vistos.contains(k));
        self.estaticos.retain(|k, _| vistos.contains(k));
        Ok(procesos)
    }

    fn version_de(&mut self, ruta: &str) -> win::version::InfoVersion {
        if let Some(v) = self.cache_version.get(ruta) {
            return v.clone();
        }
        // Tope de seguridad: si se acumulan demasiadas rutas, se empieza de
        // nuevo en lugar de crecer sin límite.
        if self.cache_version.len() > 4000 {
            self.cache_version.clear();
        }
        let v = win::version::leer(ruta);
        self.cache_version.insert(ruta.to_string(), v.clone());
        v
    }

    fn muestrear_red(&mut self, segundos: Option<f64>) -> Vec<Adaptador> {
        let Ok(lista) = win::red::adaptadores() else {
            // Sin lectura no hay con qué comparar la próxima vez.
            self.red_anterior.clear();
            return Vec::new();
        };
        let mut salida = Vec::with_capacity(lista.len());
        let mut nuevo = HashMap::with_capacity(lista.len());
        for a in lista {
            let (rx, tx) = match (segundos, self.red_anterior.get(&a.id)) {
                (Some(s), Some(&(r0, t0))) => (tasa(r0, a.recibidos, s), tasa(t0, a.enviados, s)),
                _ => (0.0, 0.0),
            };
            nuevo.insert(a.id, (a.recibidos, a.enviados));
            salida.push(Adaptador {
                nombre: a.nombre,
                descripcion: a.descripcion,
                conectado: a.conectado,
                recibidos: a.recibidos,
                enviados: a.enviados,
                rx_bps: rx,
                tx_bps: tx,
            });
        }
        self.red_anterior = nuevo;
        salida
    }
}

/// % de CPU del sistema entre dos lecturas de `GetSystemTimes`.
pub fn cpu_sistema(a: &win::sistema::TiemposCpu, b: &win::sistema::TiemposCpu) -> f64 {
    let inactivo = b.inactivo.saturating_sub(a.inactivo) as f64;
    // El tiempo de núcleo ya incluye el inactivo: total = núcleo + usuario.
    let total = (b.nucleo.saturating_sub(a.nucleo) + b.usuario.saturating_sub(a.usuario)) as f64;
    if total <= 0.0 {
        return 0.0;
    }
    ((total - inactivo) / total * 100.0).clamp(0.0, 100.0)
}

/// % de CPU de un proceso: tiempo de CPU usado dividido por el tiempo
/// disponible (segundos transcurridos × número de procesadores).
pub fn cpu_proceso(antes_100ns: u64, despues_100ns: u64, segundos: f64, n_cpu: u32) -> f64 {
    let usado_s = despues_100ns.saturating_sub(antes_100ns) as f64 / 1e7;
    let disponible_s = segundos * f64::from(n_cpu.max(1));
    if disponible_s <= 0.0 {
        return 0.0;
    }
    (usado_s / disponible_s * 100.0).clamp(0.0, 100.0)
}

/// Diferencia de un contador acumulado convertida a "por segundo".
pub fn tasa(antes: u64, despues: u64, segundos: f64) -> f64 {
    if segundos <= 0.0 {
        return 0.0;
    }
    despues.saturating_sub(antes) as f64 / segundos
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use win::sistema::TiemposCpu;

    #[test]
    fn cpu_sistema_medio_ocupado() {
        let a = TiemposCpu::default();
        // 100 de núcleo (60 inactivo) + 100 de usuario = 200; ocupado 140.
        let b = TiemposCpu {
            inactivo: 60,
            nucleo: 100,
            usuario: 100,
        };
        assert!((cpu_sistema(&a, &b) - 70.0).abs() < 1e-9);
    }

    #[test]
    fn cpu_proceso_un_nucleo_de_cuatro() {
        // 1 s de CPU en 1 s con 4 procesadores = 25 %.
        assert!((cpu_proceso(0, 10_000_000, 1.0, 4) - 25.0).abs() < 1e-9);
        // Contador que retrocede (no debería pasar): 0, no negativo.
        assert_eq!(cpu_proceso(5, 1, 1.0, 4), 0.0);
    }

    #[test]
    fn tasa_por_segundo() {
        assert_eq!(tasa(1000, 3000, 2.0), 1000.0);
        assert_eq!(tasa(1000, 3000, 0.0), 0.0);
    }
}

//! Arranque y parada de Vigía: crea el estado compartido y los hilos.
//!
//! Hilos:
//! - **colector**: cada `intervalo` toma una muestra, la pasa al historial
//!   y guarda los minutos cerrados en SQLite.
//! - **servidor**: atiende la interfaz web en 127.0.0.1.
//!
//! Se comunican a través de [`Estado`], protegido con `Mutex`.

use crate::base;
use crate::colector::{Colector, Muestra};
use crate::error::{Resultado, error};
use crate::historial::{self, Historial};
use crate::win;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Puerto por omisión. Si está ocupado se usa uno libre cualquiera.
pub const PUERTO_POR_OMISION: u16 = 47_613;

/// Configuración de arranque.
#[derive(Debug, Clone)]
pub struct Config {
    /// Carpeta de datos (base de datos y archivo de estado). `None` = base
    /// en memoria, sin escribir nada (pruebas).
    pub dir_datos: Option<PathBuf>,
    /// 0 = cualquier puerto libre.
    pub puerto: u16,
    pub intervalo: Duration,
    /// Muestras en vivo que se conservan (10 min a 2 s = 300).
    pub capacidad_vivo: usize,
    pub retencion_dias: i64,
    pub tope_bytes: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dir_datos: None,
            puerto: PUERTO_POR_OMISION,
            intervalo: Duration::from_secs(2),
            capacidad_vivo: 300,
            retencion_dias: 30,
            tope_bytes: 200 * 1024 * 1024,
        }
    }
}

/// Estado compartido entre hilos.
pub struct Estado {
    pub config: Config,
    pub token: String,
    pub puerto: u16,
    pub inicio_ms: i64,
    pub historial: Mutex<Historial>,
    pub ultima: Mutex<Option<Arc<Muestra>>>,
    pub base: Mutex<Connection>,
    /// Análisis de disco: progreso del que está en curso y pedido de cancelar.
    pub disco: EstadoDisco,
    /// Reglas de avisos vigentes (copia en memoria de lo guardado en la base).
    pub reglas: Mutex<crate::avisos::Reglas>,
    detener: AtomicBool,
}

#[derive(Default)]
pub struct EstadoDisco {
    pub progreso: Mutex<crate::disco::Progreso>,
    pub cancelar: AtomicBool,
}

impl Estado {
    pub fn pedir_detener(&self) {
        self.detener.store(true, Ordering::SeqCst);
    }
    pub fn detenido(&self) -> bool {
        self.detener.load(Ordering::SeqCst) || win::sistema_extra::cierre_pedido()
    }

    /// Registra un aviso en la base. Es el único camino por el que pasan
    /// todos los avisos (reglas, inicio, puertos).
    pub fn emitir_aviso(&self, ts: i64, tipo: &str, titulo: &str, detalle: &str) -> Resultado<()> {
        crate::avisos::registrar(&bloquear(&self.base), ts, tipo, titulo, detalle)?;
        Ok(())
    }

    /// Empieza un análisis de disco en un hilo propio. Falla si ya hay uno.
    pub fn iniciar_analisis(self: &Arc<Self>, raiz: String) -> Resultado<()> {
        {
            let mut p = bloquear(&self.disco.progreso);
            if p.en_curso {
                return Err(error("Ya hay un análisis en curso"));
            }
            *p = crate::disco::Progreso {
                en_curso: true,
                raiz: raiz.clone(),
                inicio_ms: ahora_ms(),
                ultimo_id: p.ultimo_id,
                ..Default::default()
            };
        }
        self.disco.cancelar.store(false, Ordering::SeqCst);
        let estado = Arc::clone(self);
        let lanzado = std::thread::Builder::new()
            .name("disco".into())
            .spawn(move || {
                let a = crate::disco::analizar(
                    &raiz,
                    ahora_ms,
                    &estado.disco.cancelar,
                    &estado.disco.progreso,
                );
                let guardado = crate::disco::guardar(&bloquear(&estado.base), &a);
                let mut p = bloquear(&estado.disco.progreso);
                p.en_curso = false;
                p.carpetas = a.carpetas;
                p.archivos = a.archivos;
                p.bytes = a.total;
                p.errores = a.errores;
                match guardado {
                    Ok(id) => p.ultimo_id = Some(id),
                    Err(e) => p.fallo = Some(e.to_string()),
                }
            });
        if let Err(e) = lanzado {
            bloquear(&self.disco.progreso).en_curso = false;
            return Err(e.into());
        }
        Ok(())
    }
}

/// Bloquea un `Mutex` aunque otro hilo haya entrado en pánico con él
/// tomado ("envenenado"): preferimos seguir mostrando datos a caernos.
pub fn bloquear<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Hora actual en ms desde 1970 (UTC).
pub fn ahora_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Lo que se escribe en `datos/vigia_estado.json` para que
/// `vigia --detener` y un segundo arranque encuentren a esta instancia.
#[derive(Debug, Serialize, Deserialize)]
pub struct ArchivoEstado {
    pub puerto: u16,
    pub token: String,
    pub pid: u32,
}

pub fn ruta_archivo_estado(dir: &Path) -> PathBuf {
    dir.join("vigia_estado.json")
}

/// Vigía en marcha.
pub struct App {
    estado: Arc<Estado>,
    hilos: Vec<JoinHandle<()>>,
}

/// Crea el token secreto: 32 bytes aleatorios en hexadecimal.
fn nuevo_token() -> Resultado<String> {
    let mut b = [0u8; 32];
    win::sistema_extra::aleatorio(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

pub fn iniciar(config: Config) -> Resultado<App> {
    let conn = match &config.dir_datos {
        Some(d) => base::abrir(&d.join("vigia.db"))?,
        None => base::en_memoria()?,
    };
    let reglas = crate::avisos::leer_reglas(&conn)?;
    let token = nuevo_token()?;
    // Escuchar SOLO en 127.0.0.1: ningún otro equipo de la red puede
    // conectarse. Si el puerto está ocupado, pedimos uno libre (0).
    let servidor = tiny_http::Server::http(("127.0.0.1", config.puerto))
        .or_else(|_| tiny_http::Server::http(("127.0.0.1", 0)))
        .map_err(|e| error(format!("no se pudo abrir el servidor: {e}")))?;
    let puerto = servidor
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| error("el servidor no tiene dirección IP"))?;

    let estado = Arc::new(Estado {
        historial: Mutex::new(Historial::new(config.capacidad_vivo)),
        config,
        token,
        puerto,
        inicio_ms: ahora_ms(),
        ultima: Mutex::new(None),
        base: Mutex::new(conn),
        disco: EstadoDisco::default(),
        reglas: Mutex::new(reglas),
        detener: AtomicBool::new(false),
    });

    if let Some(dir) = &estado.config.dir_datos {
        let archivo = ArchivoEstado {
            puerto,
            token: estado.token.clone(),
            pid: win::proceso::pid_propio(),
        };
        std::fs::write(ruta_archivo_estado(dir), serde_json::to_vec(&archivo)?)?;
    }

    let mut hilos = Vec::new();
    let e = Arc::clone(&estado);
    hilos.push(
        std::thread::Builder::new()
            .name("colector".into())
            .spawn(move || bucle_colector(&e))?,
    );
    let e = Arc::clone(&estado);
    hilos.push(
        std::thread::Builder::new()
            .name("servidor".into())
            .spawn(move || crate::servidor::atender(&servidor, &e))?,
    );
    let e = Arc::clone(&estado);
    hilos.push(
        std::thread::Builder::new()
            .name("vigilante".into())
            .spawn(move || bucle_vigilante(&e))?,
    );
    Ok(App { estado, hilos })
}

/// Cada minuto revisa inicio y puertos en escucha y registra avisos.
/// La primera revisión espera unos segundos para tener nombres de procesos.
fn bucle_vigilante(estado: &Estado) {
    let mut proxima = Instant::now() + Duration::from_secs(5);
    while !estado.detenido() {
        if Instant::now() >= proxima {
            proxima = Instant::now() + Duration::from_secs(60);
            if let Err(e) = revisar_cambios(estado) {
                eprintln!("[vigía] error al revisar inicio y puertos: {e}");
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Una revisión de inicio y puertos (también la usa la API para forzarla).
pub fn revisar_cambios(estado: &Estado) -> Resultado<usize> {
    let entradas = crate::inicio::leer(win::version::leer);
    let procesos = bloquear(&estado.ultima)
        .as_ref()
        .map(|m| m.procesos.clone())
        .unwrap_or_default();
    let crudas = win::red::conexiones()?;
    let puertos = crate::conexiones::puertos_en_escucha(&crudas, &procesos);
    let conn = bloquear(&estado.base);
    let mut cambios = crate::vigilante::revisar_inicio(&conn, &entradas)?;
    cambios.extend(crate::vigilante::revisar_puertos(&conn, &puertos)?);
    drop(conn);
    let ts = ahora_ms();
    let reglas = bloquear(&estado.reglas).clone();
    // La referencia se actualiza siempre; solo el aviso depende de la regla.
    let mut emitidos = 0;
    for c in cambios.iter().filter(|c| reglas.permite(c.tipo)) {
        estado.emitir_aviso(ts, c.tipo, &c.titulo, &c.detalle)?;
        emitidos += 1;
    }
    Ok(emitidos)
}

impl App {
    pub fn puerto(&self) -> u16 {
        self.estado.puerto
    }
    pub fn token(&self) -> &str {
        &self.estado.token
    }
    pub fn estado(&self) -> &Arc<Estado> {
        &self.estado
    }
    /// URL para abrir la interfaz; el token va después de `#`, que el
    /// navegador nunca envía al servidor ni guarda en registros.
    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/#token={}",
            self.estado.puerto, self.estado.token
        )
    }

    /// Espera hasta que alguien pida detener (API o cierre de consola) y
    /// luego detiene todo ordenadamente.
    pub fn esperar(self) {
        while !self.estado.detenido() {
            std::thread::sleep(Duration::from_millis(200));
        }
        self.detener();
    }

    /// Detiene los hilos, guarda el minuto en curso y borra el archivo de
    /// estado.
    pub fn detener(self) {
        self.estado.pedir_detener();
        self.estado.disco.cancelar.store(true, Ordering::SeqCst);
        for h in self.hilos {
            let _ = h.join();
        }
        if let Some(r) = bloquear(&self.estado.historial).cerrar() {
            let _ = historial::guardar_minuto(&mut bloquear(&self.estado.base), &r);
        }
        if let Some(dir) = &self.estado.config.dir_datos {
            let _ = std::fs::remove_file(ruta_archivo_estado(dir));
        }
        win::sistema_extra::marcar_cierre_listo();
    }
}

fn bucle_colector(estado: &Estado) {
    let mut colector = Colector::new();
    let mut motor = crate::avisos::Motor::default();
    let intervalo = estado.config.intervalo;
    // La primera muestra solo sirve de referencia: sus tasas (CPU %, bytes/s)
    // valen 0 porque no hay con qué comparar. No se registra; la siguiente
    // llega antes (a lo sumo 1 s) para que la interfaz tenga datos pronto.
    if let Err(e) = colector.muestrear(ahora_ms()) {
        eprintln!("[vigía] error al tomar la primera muestra: {e}");
    }
    let mut proxima = Instant::now() + intervalo.min(Duration::from_secs(1));
    let mut proxima_retencion = Instant::now();
    while !estado.detenido() {
        let ahora = Instant::now();
        if ahora >= proxima {
            // Si nos atrasamos (p. ej. el equipo despertó de suspensión), no
            // intentamos "recuperar" muestras: seguimos desde ahora.
            proxima = (proxima + intervalo).max(ahora);
            tomar_muestra(estado, &mut colector, &mut motor);
        }
        if ahora >= proxima_retencion {
            proxima_retencion = ahora + Duration::from_secs(3600);
            let conn = bloquear(&estado.base);
            if let Err(e) = historial::aplicar_retencion(
                &conn,
                ahora_ms(),
                estado.config.retencion_dias,
                estado.config.tope_bytes,
            ) {
                eprintln!("[vigía] error en la retención: {e}");
            }
        }
        let espera = proxima.saturating_duration_since(Instant::now());
        std::thread::sleep(espera.min(Duration::from_millis(100)));
    }
}

fn tomar_muestra(estado: &Estado, colector: &mut Colector, motor: &mut crate::avisos::Motor) {
    let muestra = match colector.muestrear(ahora_ms()) {
        Ok(m) => Arc::new(m),
        Err(e) => {
            eprintln!("[vigía] error al tomar la muestra: {e}");
            return;
        }
    };
    let cerrado = bloquear(&estado.historial).registrar(&muestra);
    let reglas = bloquear(&estado.reglas).clone();
    for a in motor.evaluar(&reglas, &muestra) {
        if let Err(e) = estado.emitir_aviso(muestra.ts, a.tipo, &a.titulo, &a.detalle) {
            eprintln!("[vigía] error al registrar un aviso: {e}");
        }
    }
    *bloquear(&estado.ultima) = Some(muestra);
    if let Some(r) = cerrado
        && let Err(e) = historial::guardar_minuto(&mut bloquear(&estado.base), &r)
    {
        eprintln!("[vigía] error al guardar el minuto: {e}");
    }
}

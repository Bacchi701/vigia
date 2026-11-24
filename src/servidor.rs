//! Servidor HTTP local: la página y la API JSON.
//!
//! ## Seguridad
//! La API muestra datos sensibles (procesos, rutas, conexiones), así que:
//! 1. Solo escucha en `127.0.0.1` (ver `app.rs`): nadie de la red llega.
//! 2. Rechaza peticiones cuyo `Host` no sea `127.0.0.1:<puerto>` o
//!    `localhost:<puerto>`. Esto frena el *DNS rebinding*: una página
//!    maliciosa que hace que su dominio apunte a 127.0.0.1 enviaría su
//!    propio nombre en `Host`.
//! 3. Rechaza peticiones con un `Origin` distinto del nuestro: otra página
//!    abierta en el navegador no puede llamar a la API.
//! 4. Toda ruta `/api/...` exige el encabezado `X-Vigia-Token` con el token
//!    secreto generado al iniciar. La página lo recibe en la URL (después de
//!    `#`, que nunca viaja al servidor) y lo envía en cada petición.

use crate::app::{Estado, ahora_ms, bloquear};
use crate::error::{Error, Resultado, error};
use crate::historial::{self, MINUTO_MS, Orden};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tiny_http::{Header, Request, Server};

/// La interfaz completa, incluida dentro del ejecutable al compilar.
const PAGINA: &str = include_str!("../ui/index.html");

/// Tamaño máximo aceptado para el cuerpo de una petición.
const MAX_CUERPO: u64 = 64 * 1024;

/// Peticiones atendidas a la vez como máximo.
const MAX_HILOS: usize = 16;

/// Respuesta antes de convertirla a `tiny_http`.
pub struct Respuesta {
    pub estado: u16,
    pub tipo: &'static str,
    pub cuerpo: Vec<u8>,
    /// Nombre de archivo sugerido para descargas (CSV).
    pub descarga: Option<String>,
}

impl Respuesta {
    fn json<T: Serialize>(valor: &T) -> Respuesta {
        match serde_json::to_vec(valor) {
            Ok(cuerpo) => Respuesta {
                estado: 200,
                tipo: "application/json; charset=utf-8",
                cuerpo,
                descarga: None,
            },
            Err(e) => Respuesta::error(500, &format!("JSON: {e}")),
        }
    }
    fn error(estado: u16, mensaje: &str) -> Respuesta {
        Respuesta {
            estado,
            tipo: "application/json; charset=utf-8",
            cuerpo: serde_json::json!({ "error": mensaje })
                .to_string()
                .into_bytes(),
            descarga: None,
        }
    }
}

/// Bucle del hilo servidor. Espera peticiones en intervalos cortos para
/// poder notar a tiempo que hay que detenerse.
///
/// Cada petición se atiende en un hilo propio (con un máximo de
/// `MAX_HILOS` a la vez): así un cliente lento o una consulta larga no
/// bloquean la interfaz ni el cierre de Vigía.
pub fn atender(servidor: &Server, estado: &Arc<Estado>) {
    let activos = Arc::new(AtomicUsize::new(0));
    while !estado.detenido() {
        match servidor.recv_timeout(Duration::from_millis(250)) {
            Ok(Some(peticion)) => {
                if activos.load(Ordering::SeqCst) >= MAX_HILOS {
                    let _ = peticion
                        .respond(tiny_http::Response::from_string("Ocupado").with_status_code(503));
                    continue;
                }
                activos.fetch_add(1, Ordering::SeqCst);
                let (e, a) = (Arc::clone(estado), Arc::clone(&activos));
                let lanzado =
                    std::thread::Builder::new()
                        .name("peticion".into())
                        .spawn(move || {
                            manejar(peticion, &e);
                            a.fetch_sub(1, Ordering::SeqCst);
                        });
                if lanzado.is_err() {
                    activos.fetch_sub(1, Ordering::SeqCst);
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("[vigía] error del servidor: {e}");
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

/// Datos de la petición que necesita el enrutador.
pub struct Peticion<'a> {
    pub metodo: &'a str,
    pub ruta: &'a str,
    pub consulta: HashMap<String, String>,
    pub host: Option<&'a str>,
    pub origen: Option<&'a str>,
    pub token: Option<&'a str>,
    pub cuerpo: String,
}

fn manejar(mut peticion: Request, estado: &Arc<Estado>) {
    let metodo = peticion.method().as_str().to_string();
    let url = peticion.url().to_string();
    let (ruta, consulta) = url.split_once('?').unwrap_or((&url, ""));
    let encabezado = |nombre: &'static str| -> Option<String> {
        peticion
            .headers()
            .iter()
            .find(|h| h.field.equiv(nombre))
            .map(|h| h.value.as_str().to_string())
    };
    let host = encabezado("Host");
    let origen = encabezado("Origin");
    let token = encabezado("X-Vigia-Token");
    let mut p = Peticion {
        metodo: &metodo,
        ruta,
        consulta: parsear_consulta(consulta),
        host: host.as_deref(),
        origen: origen.as_deref(),
        token: token.as_deref(),
        cuerpo: String::new(),
    };
    // El cuerpo se lee solo si la petición pasó las comprobaciones de
    // seguridad y es un POST: nadie sin token puede hacernos esperar datos.
    let r = match rechazo(&p, estado) {
        Some(r) => r,
        None => {
            if p.metodo == "POST" {
                let _ = peticion
                    .as_reader()
                    .take(MAX_CUERPO)
                    .read_to_string(&mut p.cuerpo);
            }
            rutear(&p, estado)
        }
    };

    let mut respuesta = tiny_http::Response::from_data(r.cuerpo).with_status_code(r.estado);
    let mut encabezados = vec![
        ("Content-Type", r.tipo.to_string()),
        ("X-Content-Type-Options", "nosniff".into()),
        ("Referrer-Policy", "no-referrer".into()),
        ("Cache-Control", "no-store".into()),
        // La página no puede cargar nada externo ni ser incrustada en otra.
        (
            "Content-Security-Policy",
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
             img-src data:; connect-src 'self'; base-uri 'none'; form-action 'none'; \
             frame-ancestors 'none'"
                .into(),
        ),
        ("X-Frame-Options", "DENY".into()),
    ];
    if let Some(nombre) = r.descarga {
        encabezados.push((
            "Content-Disposition",
            format!("attachment; filename=\"{nombre}\""),
        ));
    }
    for (k, v) in encabezados {
        if let Ok(h) = Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            respuesta.add_header(h);
        }
    }
    let _ = peticion.respond(respuesta);
}

/// ¿El `Host` es exactamente el nuestro?
fn host_valido(host: Option<&str>, puerto: u16) -> bool {
    let Some(h) = host else { return false };
    h == format!("127.0.0.1:{puerto}") || h == format!("localhost:{puerto}")
}

/// ¿El `Origin` (si viene) es el nuestro?
fn origen_valido(origen: Option<&str>, puerto: u16) -> bool {
    match origen {
        None => true, // Navegación directa o petición del mismo origen (GET).
        Some(o) => {
            o == format!("http://127.0.0.1:{puerto}") || o == format!("http://localhost:{puerto}")
        }
    }
}

/// Compara en tiempo constante: el tiempo de respuesta no revela cuántos
/// caracteres del token se acertaron.
fn token_valido(dado: Option<&str>, correcto: &str) -> bool {
    let Some(d) = dado else { return false };
    if d.len() != correcto.len() {
        return false;
    }
    d.bytes()
        .zip(correcto.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Comprobaciones de seguridad. Devuelve `Some(respuesta de error)` si la
/// petición debe rechazarse.
fn rechazo(p: &Peticion, estado: &Estado) -> Option<Respuesta> {
    if !host_valido(p.host, estado.puerto) {
        return Some(Respuesta::error(403, "Host no permitido"));
    }
    if !origen_valido(p.origen, estado.puerto) {
        return Some(Respuesta::error(403, "Origen no permitido"));
    }
    if p.ruta.starts_with("/api/") && !token_valido(p.token, &estado.token) {
        return Some(Respuesta::error(401, "Falta el token o no es válido"));
    }
    None
}

/// Decide la respuesta. Separado de `manejar` para que sea fácil de leer.
pub fn rutear(p: &Peticion, estado: &Arc<Estado>) -> Respuesta {
    if let Some(r) = rechazo(p, estado) {
        return r;
    }
    if !p.ruta.starts_with("/api/") {
        return match (p.metodo, p.ruta) {
            ("GET", "/") | ("GET", "/index.html") => Respuesta {
                estado: 200,
                tipo: "text/html; charset=utf-8",
                cuerpo: PAGINA.as_bytes().to_vec(),
                descarga: None,
            },
            _ => Respuesta::error(404, "No existe"),
        };
    }
    match api(p, estado) {
        Ok(r) => r,
        Err(Error::Mensaje(m)) => Respuesta::error(400, &m),
        Err(e) => Respuesta::error(500, &e.to_string()),
    }
}

fn api(p: &Peticion, estado: &Arc<Estado>) -> Resultado<Respuesta> {
    let num = |clave: &str, omision: i64| -> Resultado<i64> {
        match p.consulta.get(clave) {
            None => Ok(omision),
            // Se limita a ±2^52 (unos 140 000 años en ms): ninguna suma o
            // resta posterior entre dos de estos valores puede desbordarse.
            Some(v) => v
                .parse::<i64>()
                .map(|n| n.clamp(-(1 << 52), 1 << 52))
                .map_err(|_| error(format!("parámetro '{clave}' inválido"))),
        }
    };
    let ahora = ahora_ms();
    Ok(match (p.metodo, p.ruta) {
        ("GET", "/api/estado") => Respuesta::json(&serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "inicio_ms": estado.inicio_ms,
            "ahora_ms": ahora,
            "pid": crate::win::proceso::pid_propio(),
            "intervalo_ms": estado.config.intervalo.as_millis() as u64,
            "datos_en_disco": estado.config.dir_datos.is_some(),
        })),
        ("GET", "/api/ahora") => {
            let ultima = bloquear(&estado.ultima).clone();
            let vivo: Vec<_> = bloquear(&estado.historial).vivo().cloned().collect();
            Respuesta::json(&serde_json::json!({ "muestra": ultima.as_deref(), "vivo": vivo }))
        }
        ("GET", "/api/proceso") => {
            let pid = num("pid", 0)? as u32;
            let inicio = num("inicio", 0)?;
            let serie = bloquear(&estado.historial).proceso(pid, inicio);
            Respuesta::json(&serde_json::json!({ "serie": serie }))
        }
        ("GET", "/api/historial/linea") => {
            let hasta = num("hasta", ahora)?;
            let desde = num("desde", hasta - 60 * MINUTO_MS)?;
            let puntos = num("puntos", 300)?.clamp(10, 2000) as usize;
            let conn = bloquear(&estado.base);
            Respuesta::json(&historial::linea_tiempo(&conn, desde, hasta, puntos)?)
        }
        ("GET", "/api/historial/momento") => {
            let ts = num("ts", ahora)?;
            let conn = bloquear(&estado.base);
            let radio = num("radio", 10 * MINUTO_MS)?;
            Respuesta::json(&historial::momento(&conn, ts, radio)?)
        }
        ("GET", "/api/apps") => {
            let hasta = num("hasta", ahora)?;
            let desde = num("desde", hasta - historial::DIA_MS)?;
            let limite = num("limite", 30)?.clamp(1, 500) as usize;
            let orden = Orden::desde_texto(p.consulta.get("orden").map_or("cpu", String::as_str));
            let conn = bloquear(&estado.base);
            let apps = historial::top_apps(&conn, desde, hasta, orden, limite)?;
            let minutos = historial::minutos_con_datos(&conn, desde, hasta)?;
            Respuesta::json(&serde_json::json!({ "apps": apps, "minutos": minutos }))
        }
        // ---------- Disco ----------
        ("POST", "/api/disco/analizar") => {
            let ruta = cuerpo_json(p)?["ruta"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let raiz = crate::disco::normalizar_raiz(&ruta)?;
            estado.iniciar_analisis(raiz)?;
            Respuesta::json(&serde_json::json!({ "ok": true }))
        }
        ("GET", "/api/disco/progreso") => Respuesta::json(&*bloquear(&estado.disco.progreso)),
        ("POST", "/api/disco/cancelar") => {
            estado.disco.cancelar.store(true, Ordering::SeqCst);
            Respuesta::json(&serde_json::json!({ "ok": true }))
        }
        ("GET", "/api/disco/lista") => {
            Respuesta::json(&crate::disco::lista(&bloquear(&estado.base))?)
        }
        ("GET", "/api/disco/analisis") => {
            let id = p.consulta.get("id").and_then(|x| x.parse().ok());
            let conn = bloquear(&estado.base);
            let a = crate::disco::leer(&conn, id)?;
            let previo = match &a {
                Some(a) => crate::disco::anterior(&conn, a)?,
                None => None,
            };
            let cambios = match (&previo, &a) {
                (Some(b), Some(a)) => {
                    let mut c = crate::disco::comparar(b, a);
                    c.truncate(60);
                    Some(c)
                }
                _ => None,
            };
            Respuesta::json(&serde_json::json!({
                "analisis": a,
                "anterior": previo.map(|b| serde_json::json!({ "id": b.id, "fin_ms": b.fin_ms, "total": b.total })),
                "cambios": cambios,
            }))
        }
        ("POST", "/api/disco/abrir") => {
            // Solo abre rutas que aparecen en un análisis guardado: la API no
            // sirve para abrir rutas arbitrarias en el Explorador.
            let cuerpo = cuerpo_json(p)?;
            let id = cuerpo["id"].as_i64().ok_or_else(|| error("falta id"))?;
            let ruta = cuerpo["ruta"].as_str().unwrap_or_default();
            let a = crate::disco::leer(&bloquear(&estado.base), Some(id))?
                .ok_or_else(|| error("no existe ese análisis"))?;
            if !crate::disco::contiene_ruta(&a, ruta) {
                return Err(error("Esa ruta no está en el análisis"));
            }
            crate::win::sistema_extra::mostrar_en_explorador(ruta)?;
            Respuesta::json(&serde_json::json!({ "ok": true }))
        }
        // ---------- Inicio, conexiones y avisos ----------
        ("GET", "/api/inicio") => {
            let entradas = crate::inicio::leer(crate::win::version::leer);
            Respuesta::json(&entradas)
        }
        ("GET", "/api/conexiones") => {
            let procesos = bloquear(&estado.ultima)
                .as_ref()
                .map(|m| m.procesos.clone())
                .unwrap_or_default();
            let crudas = crate::win::red::conexiones()?;
            Respuesta::json(&serde_json::json!({
                "conexiones": crate::conexiones::armar(&crudas, &procesos),
                "escucha": crate::conexiones::puertos_en_escucha(&crudas, &procesos),
            }))
        }
        ("POST", "/api/revisar") => {
            let n = crate::app::revisar_cambios(estado)?;
            Respuesta::json(&serde_json::json!({ "cambios": n }))
        }
        ("GET", "/api/avisos") => {
            let limite = num("limite", 200)?.clamp(1, 2000) as usize;
            let conn = bloquear(&estado.base);
            Respuesta::json(&serde_json::json!({
                "avisos": crate::avisos::listar(&conn, limite)?,
                "sin_ver": crate::avisos::sin_ver(&conn)?,
            }))
        }
        ("GET", "/api/reglas") => Respuesta::json(&*bloquear(&estado.reglas)),
        ("POST", "/api/reglas") => {
            let nuevas: crate::avisos::Reglas = serde_json::from_str(&p.cuerpo)
                .map_err(|e| error(format!("Reglas inválidas: {e}")))?;
            let nuevas = nuevas.corregidas();
            crate::avisos::guardar_reglas(&bloquear(&estado.base), &nuevas)?;
            *bloquear(&estado.reglas) = nuevas.clone();
            Respuesta::json(&nuevas)
        }
        ("POST", "/api/avisos/vistos") => {
            let n = crate::avisos::marcar_vistos(&bloquear(&estado.base))?;
            Respuesta::json(&serde_json::json!({ "marcados": n }))
        }
        ("POST", "/api/apagar") => {
            estado.pedir_detener();
            Respuesta::json(&serde_json::json!({ "ok": true }))
        }
        _ => Respuesta::error(404, "No existe esa ruta de la API"),
    })
}

/// Lee el cuerpo de la petición como JSON.
fn cuerpo_json(p: &Peticion) -> Resultado<serde_json::Value> {
    serde_json::from_str(&p.cuerpo).map_err(|_| error("El cuerpo debe ser JSON"))
}

/// Convierte `a=1&b=hola%20mundo` en un mapa.
pub fn parsear_consulta(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|s| !s.is_empty())
        .map(|par| {
            let (k, v) = par.split_once('=').unwrap_or((par, ""));
            (decodificar(k), decodificar(v))
        })
        .collect()
}

/// Decodifica `%XX` y `+` de una URL.
pub fn decodificar(s: &str) -> String {
    let b = s.as_bytes();
    let mut salida = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => salida.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        salida.push(v);
                        i += 2;
                    }
                    None => salida.push(b'%'),
                }
            }
            x => salida.push(x),
        }
        i += 1;
    }
    String::from_utf8_lossy(&salida).to_string()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn decodifica_url() {
        assert_eq!(decodificar("C%3A%5CUsers+x%C3%AD"), "C:\\Users xí");
        assert_eq!(decodificar("100%"), "100%");
        assert_eq!(decodificar("%zz"), "%zz");
        let q = parsear_consulta("a=1&b=dos%20tres&c");
        assert_eq!(q["b"], "dos tres");
        assert_eq!(q["c"], "");
    }

    #[test]
    fn host_origen_token() {
        assert!(host_valido(Some("127.0.0.1:80"), 80));
        assert!(host_valido(Some("localhost:80"), 80));
        assert!(!host_valido(Some("malo.com:80"), 80));
        assert!(!host_valido(Some("127.0.0.1:81"), 80));
        assert!(!host_valido(None, 80));
        assert!(origen_valido(None, 80));
        assert!(origen_valido(Some("http://127.0.0.1:80"), 80));
        assert!(!origen_valido(Some("http://malo.com"), 80));
        assert!(!origen_valido(Some("null"), 80));
        assert!(token_valido(Some("abc"), "abc"));
        assert!(!token_valido(Some("abd"), "abc"));
        assert!(!token_valido(Some("ab"), "abc"));
        assert!(!token_valido(None, "abc"));
    }
}

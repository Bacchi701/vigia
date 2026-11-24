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
use std::time::Duration;
use tiny_http::{Header, Request, Server};

/// La interfaz completa, incluida dentro del ejecutable al compilar.
const PAGINA: &str = include_str!("../ui/index.html");

/// Tamaño máximo aceptado para el cuerpo de una petición.
const MAX_CUERPO: u64 = 64 * 1024;

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
pub fn atender(servidor: &Server, estado: &Estado) {
    while !estado.detenido() {
        match servidor.recv_timeout(Duration::from_millis(250)) {
            Ok(Some(peticion)) => manejar(peticion, estado),
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

fn manejar(mut peticion: Request, estado: &Estado) {
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
    let mut cuerpo = String::new();
    let _ = peticion
        .as_reader()
        .take(MAX_CUERPO)
        .read_to_string(&mut cuerpo);

    let p = Peticion {
        metodo: &metodo,
        ruta,
        consulta: parsear_consulta(consulta),
        host: host.as_deref(),
        origen: origen.as_deref(),
        token: token.as_deref(),
        cuerpo,
    };
    let r = rutear(&p, estado);

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

/// Decide la respuesta. Separado de `manejar` para que sea fácil de leer.
pub fn rutear(p: &Peticion, estado: &Estado) -> Respuesta {
    if !host_valido(p.host, estado.puerto) {
        return Respuesta::error(403, "Host no permitido");
    }
    if !origen_valido(p.origen, estado.puerto) {
        return Respuesta::error(403, "Origen no permitido");
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
    if !token_valido(p.token, &estado.token) {
        return Respuesta::error(401, "Falta el token o no es válido");
    }
    match api(p, estado) {
        Ok(r) => r,
        Err(Error::Mensaje(m)) => Respuesta::error(400, &m),
        Err(e) => Respuesta::error(500, &e.to_string()),
    }
}

fn api(p: &Peticion, estado: &Estado) -> Resultado<Respuesta> {
    let num = |clave: &str, omision: i64| -> Resultado<i64> {
        match p.consulta.get(clave) {
            None => Ok(omision),
            Some(v) => v
                .parse()
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
            Respuesta::json(&historial::momento(&conn, ts)?)
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
        ("POST", "/api/apagar") => {
            estado.pedir_detener();
            Respuesta::json(&serde_json::json!({ "ok": true }))
        }
        _ => Respuesta::error(404, "No existe esa ruta de la API"),
    })
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

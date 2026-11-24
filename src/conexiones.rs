//! Conexiones de red por proceso y puertos en escucha.
//!
//! Solo se consulta **qué proceso tiene qué conexión** (tablas de Windows);
//! nunca se captura ni se lee el contenido de las conexiones. Tampoco se
//! resuelven nombres de dominio: eso enviaría consultas DNS a internet.

use crate::colector::Proceso;
use crate::win::red::{ConexionCruda, TCP_ESCUCHA};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize)]
pub struct Conexion {
    pub protocolo: &'static str,
    pub local: String,
    pub puerto_local: u16,
    pub remota: Option<String>,
    pub puerto_remoto: u16,
    pub estado: &'static str,
    /// "local" (este equipo), "red local" o "internet".
    pub alcance: &'static str,
    pub pid: u32,
    pub proceso: String,
    pub ruta: Option<String>,
}

/// Un puerto en escucha (un programa esperando conexiones).
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct PuertoEscucha {
    pub protocolo: &'static str,
    pub puerto: u16,
    pub direccion: String,
    /// `true` si otros equipos podrían conectarse (si el firewall lo
    /// permite): escucha en todas las interfaces (0.0.0.0 o ::) o en la
    /// dirección de una interfaz de red. `false` solo para 127.x / ::1.
    pub expuesto: bool,
    /// Texto para la interfaz: "toda la red", "la red de esa dirección" o
    /// "solo este equipo".
    pub accesible: &'static str,
    pub proceso: String,
    pub pid: u32,
}

impl PuertoEscucha {
    /// Clave estable para comparar revisiones: no usa el PID (cambia en
    /// cada reinicio del programa) sino el nombre del programa.
    pub fn clave(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.protocolo,
            self.puerto,
            if self.expuesto { "todas" } else { "local" },
            self.proceso.to_lowercase()
        )
    }
}

/// Nombre del estado TCP (MIB_TCP_STATE).
fn estado_tcp(e: u32) -> &'static str {
    match e {
        1 => "cerrada",
        2 => "en escucha",
        3 => "abriendo (SYN enviado)",
        4 => "abriendo (SYN recibido)",
        5 => "establecida",
        6 | 7 => "cerrando (FIN_WAIT)",
        8 => "cerrando (CLOSE_WAIT)",
        9 => "cerrando",
        10 => "cerrando (LAST_ACK)",
        11 => "esperando cierre (TIME_WAIT)",
        12 => "eliminada",
        _ => "desconocido",
    }
}

/// ¿A dónde apunta una dirección?
pub fn alcance(ip: &IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(v) => {
            if v.is_loopback() || v.is_unspecified() {
                "local"
            } else if v.is_private() || v.is_link_local() {
                "red local"
            } else {
                "internet"
            }
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return alcance(&IpAddr::V4(v4));
            }
            let s = v.segments()[0];
            if v.is_loopback() || v.is_unspecified() {
                "local"
            } else if (s & 0xffc0) == 0xfe80 || (s & 0xfe00) == 0xfc00 {
                "red local" // fe80::/10 (enlace local) y fc00::/7 (únicas locales)
            } else {
                "internet"
            }
        }
    }
}

/// Convierte las filas de Windows en conexiones con nombre de proceso.
pub fn armar(crudas: &[ConexionCruda], procesos: &[Proceso]) -> Vec<Conexion> {
    let por_pid: HashMap<u32, &Proceso> = procesos.iter().map(|p| (p.pid, p)).collect();
    crudas
        .iter()
        .map(|c| {
            let p = por_pid.get(&c.pid);
            let remota = c.remota.filter(|r| !r.is_unspecified());
            Conexion {
                protocolo: if c.tcp { "TCP" } else { "UDP" },
                local: c.local.to_string(),
                puerto_local: c.puerto_local,
                alcance: remota.as_ref().map(alcance).unwrap_or("local"),
                remota: remota.map(|r| r.to_string()),
                puerto_remoto: c.puerto_remoto,
                estado: if c.tcp {
                    estado_tcp(c.estado)
                } else {
                    "abierto"
                },
                pid: c.pid,
                proceso: nombre_proceso(c.pid, p.map(|x| x.nombre.as_str())),
                ruta: p.and_then(|x| x.ruta.clone()),
            }
        })
        .collect()
}

fn nombre_proceso(pid: u32, nombre: Option<&str>) -> String {
    match (pid, nombre) {
        (0, _) => "Sistema (inactivo)".into(),
        (4, _) => "System".into(),
        (_, Some(n)) => n.to_string(),
        _ => format!("PID {pid}"),
    }
}

/// Puertos en escucha: TCP en estado LISTEN y puertos UDP abiertos.
/// Sin repetir el mismo puerto/programa (p. ej. IPv4 e IPv6 a la vez).
pub fn puertos_en_escucha(crudas: &[ConexionCruda], procesos: &[Proceso]) -> Vec<PuertoEscucha> {
    let por_pid: HashMap<u32, &Proceso> = procesos.iter().map(|p| (p.pid, p)).collect();
    let mut vistos = HashSet::new();
    let mut lista = Vec::new();
    for c in crudas {
        if c.tcp && c.estado != TCP_ESCUCHA {
            continue;
        }
        let p = PuertoEscucha {
            protocolo: if c.tcp { "TCP" } else { "UDP" },
            puerto: c.puerto_local,
            direccion: c.local.to_string(),
            expuesto: !c.local.is_loopback(),
            accesible: if c.local.is_unspecified() {
                "toda la red"
            } else if c.local.is_loopback() {
                "solo este equipo"
            } else {
                "la red de esa dirección"
            },
            proceso: nombre_proceso(c.pid, por_pid.get(&c.pid).map(|x| x.nombre.as_str())),
            pid: c.pid,
        };
        if vistos.insert(p.clave()) {
            lista.push(p);
        }
    }
    lista.sort_by(|a, b| (a.protocolo, a.puerto).cmp(&(b.protocolo, b.puerto)));
    lista
}

/// Puertos TCP en escucha que no estaban en `conocidos`.
///
/// Solo TCP: los puertos UDP cambian constantemente (cada consulta DNS
/// abre uno), y avisar de cada uno sería ruido.
pub fn puertos_nuevos<'a>(
    conocidos: &HashSet<String>,
    actuales: &'a [PuertoEscucha],
) -> Vec<&'a PuertoEscucha> {
    actuales
        .iter()
        .filter(|p| p.protocolo == "TCP" && !conocidos.contains(&p.clave()))
        .collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn cruda(tcp: bool, local: IpAddr, puerto: u16, estado: u32, pid: u32) -> ConexionCruda {
        ConexionCruda {
            tcp,
            local,
            puerto_local: puerto,
            remota: Some(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))),
            puerto_remoto: 443,
            estado,
            pid,
        }
    }

    #[test]
    fn clasifica_alcance() {
        assert_eq!(alcance(&"127.0.0.1".parse().unwrap()), "local");
        assert_eq!(alcance(&"192.168.1.5".parse().unwrap()), "red local");
        assert_eq!(alcance(&"10.0.0.1".parse().unwrap()), "red local");
        assert_eq!(alcance(&"8.8.8.8".parse().unwrap()), "internet");
        assert_eq!(alcance(&"fe80::1".parse().unwrap()), "red local");
        assert_eq!(alcance(&"::1".parse().unwrap()), "local");
        assert_eq!(alcance(&"::ffff:192.168.0.1".parse().unwrap()), "red local");
        assert_eq!(alcance(&"2606:4700::1111".parse().unwrap()), "internet");
    }

    #[test]
    fn puertos_en_escucha_y_nuevos() {
        let todas = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        let local = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let crudas = vec![
            cruda(true, todas, 80, TCP_ESCUCHA, 10),
            cruda(true, IpAddr::V6(Ipv6Addr::UNSPECIFIED), 80, TCP_ESCUCHA, 10), // repetido v6
            cruda(true, local, 5000, TCP_ESCUCHA, 11),
            cruda(true, local, 50000, 5, 11), // establecida: no es escucha
            cruda(false, todas, 5353, 0, 12),
        ];
        let p = puertos_en_escucha(&crudas, &[]);
        assert_eq!(p.len(), 3);
        assert!(p[0].expuesto && p[0].puerto == 80);
        assert!(!p[1].expuesto && p[1].puerto == 5000);
        let conocidos: HashSet<String> = [p[0].clave()].into_iter().collect();
        let nuevos = puertos_nuevos(&conocidos, &p);
        assert_eq!(nuevos.len(), 1); // 5000; el UDP no cuenta
        assert_eq!(nuevos[0].puerto, 5000);
    }

    #[test]
    fn conexiones_reales_con_nombre_de_proceso() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto = l.local_addr().unwrap().port();
        let yo = std::process::id();
        let procesos = vec![crate::historial::pruebas::proceso(yo, "prueba.exe", 0.0, 1)];
        let crudas = crate::win::red::conexiones().unwrap();
        let p = puertos_en_escucha(&crudas, &procesos);
        let mio = p
            .iter()
            .find(|x| x.puerto == puerto && x.pid == yo)
            .unwrap();
        assert_eq!(mio.proceso, "prueba.exe");
        assert!(!mio.expuesto);
        let c = armar(&crudas, &procesos);
        assert!(
            c.iter()
                .any(|x| x.puerto_local == puerto && x.estado == "en escucha")
        );
    }
}

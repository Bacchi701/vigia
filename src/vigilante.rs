//! Revisión periódica de cambios: entradas de inicio nuevas o quitadas y
//! puertos TCP en escucha nuevos.
//!
//! La revisión anterior se guarda en la tabla `config`, así que también se
//! detectan cambios ocurridos mientras Vigía estaba cerrado. La primera vez
//! solo se guarda la lista de referencia, sin avisar.

use crate::base;
use crate::conexiones::PuertoEscucha;
use crate::error::Resultado;
use crate::inicio::{self, Entrada};
use rusqlite::Connection;
use std::collections::HashSet;

const CLAVE_INICIO: &str = "inicio_conocido";
const CLAVE_PUERTOS: &str = "puertos_conocidos";

/// Un cambio detectado, listo para convertirse en aviso.
#[derive(Debug, Clone, PartialEq)]
pub struct Cambio {
    pub tipo: &'static str,
    pub titulo: String,
    pub detalle: String,
}

/// Compara las entradas de inicio con la revisión anterior y guarda la nueva.
pub fn revisar_inicio(conn: &Connection, actuales: &[Entrada]) -> Resultado<Vec<Cambio>> {
    let anteriores: Option<Vec<Entrada>> = base::leer_config(conn, CLAVE_INICIO)?;
    base::guardar_config(conn, CLAVE_INICIO, &actuales)?;
    let Some(anteriores) = anteriores else {
        return Ok(Vec::new());
    };
    let c = inicio::comparar(&anteriores, actuales);
    let mut cambios = Vec::new();
    for e in c.nuevas {
        cambios.push(Cambio {
            tipo: "inicio_nuevo",
            titulo: format!("Nuevo programa de inicio: {}", e.nombre),
            detalle: format!(
                "{} · {}{}",
                e.origen,
                e.comando,
                e.empresa.map(|x| format!(" · {x}")).unwrap_or_default()
            ),
        });
    }
    for e in c.quitadas {
        cambios.push(Cambio {
            tipo: "inicio_quitado",
            titulo: format!("Se quitó un programa de inicio: {}", e.nombre),
            detalle: format!("{} · {}", e.origen, e.comando),
        });
    }
    Ok(cambios)
}

/// Compara los puertos en escucha con los conocidos. Los conocidos se
/// acumulan: un puerto que se cierra y se vuelve a abrir no avisa de nuevo.
pub fn revisar_puertos(conn: &Connection, actuales: &[PuertoEscucha]) -> Resultado<Vec<Cambio>> {
    let previos: Option<Vec<String>> = base::leer_config(conn, CLAVE_PUERTOS)?;
    let primera_vez = previos.is_none();
    let mut conocidos: HashSet<String> = previos.unwrap_or_default().into_iter().collect();
    let nuevos: Vec<PuertoEscucha> = crate::conexiones::puertos_nuevos(&conocidos, actuales)
        .into_iter()
        .cloned()
        .collect();
    conocidos.extend(actuales.iter().map(PuertoEscucha::clave));
    let mut lista: Vec<String> = conocidos.into_iter().collect();
    lista.sort();
    base::guardar_config(conn, CLAVE_PUERTOS, &lista)?;
    if primera_vez {
        return Ok(Vec::new());
    }
    Ok(nuevos
        .into_iter()
        .map(|p| Cambio {
            tipo: "puerto_nuevo",
            titulo: format!("{} escucha en el puerto {} {}", p.proceso, p.protocolo, p.puerto),
            detalle: if p.expuesto {
                format!(
                    "Escucha en {} (accesible desde {}): otros equipos podrían conectarse si el firewall lo permite. PID {}.",
                    p.direccion, p.accesible, p.pid
                )
            } else {
                format!("Solo en este equipo ({}). PID {}.", p.direccion, p.pid)
            },
        })
        .collect())
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn entrada(nombre: &str) -> Entrada {
        Entrada {
            origen: "HKCU\\Run".into(),
            nombre: nombre.into(),
            comando: format!("C:\\{nombre}.exe"),
            ejecutable: None,
            existe: None,
            habilitada: Some(true),
            descripcion: None,
            empresa: None,
        }
    }

    fn puerto(n: u16, proceso: &str) -> PuertoEscucha {
        PuertoEscucha {
            protocolo: "TCP",
            puerto: n,
            direccion: "0.0.0.0".into(),
            expuesto: true,
            accesible: "toda la red",
            proceso: proceso.into(),
            pid: 1,
        }
    }

    #[test]
    fn inicio_primera_vez_sin_avisos_luego_detecta_cambios() {
        let c = base::en_memoria().unwrap();
        assert!(revisar_inicio(&c, &[entrada("a")]).unwrap().is_empty());
        assert!(revisar_inicio(&c, &[entrada("a")]).unwrap().is_empty());
        let cambios = revisar_inicio(&c, &[entrada("b")]).unwrap();
        assert_eq!(cambios.len(), 2);
        assert!(
            cambios
                .iter()
                .any(|x| x.tipo == "inicio_nuevo" && x.titulo.contains('b'))
        );
        assert!(
            cambios
                .iter()
                .any(|x| x.tipo == "inicio_quitado" && x.titulo.contains('a'))
        );
    }

    #[test]
    fn puertos_nuevos_avisan_una_sola_vez() {
        let c = base::en_memoria().unwrap();
        assert!(
            revisar_puertos(&c, &[puerto(80, "web.exe")])
                .unwrap()
                .is_empty()
        );
        let cambios = revisar_puertos(&c, &[puerto(80, "web.exe"), puerto(9000, "x.exe")]).unwrap();
        assert_eq!(cambios.len(), 1);
        assert!(cambios[0].titulo.contains("9000"));
        // Se cierra y se vuelve a abrir: ya es conocido.
        assert!(revisar_puertos(&c, &[]).unwrap().is_empty());
        assert!(
            revisar_puertos(&c, &[puerto(9000, "x.exe")])
                .unwrap()
                .is_empty()
        );
    }
}

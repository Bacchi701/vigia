//! Cliente HTTP mínimo para hablar con una instancia de Vigía en
//! 127.0.0.1 (lo usan `vigia --detener` y las pruebas).
//!
//! Se escribe a mano sobre `TcpStream` porque solo hace falta una petición
//! simple; así no se agrega otra dependencia.

use crate::error::{Resultado, error};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Respuesta: código de estado, encabezados (en minúsculas) y cuerpo.
#[derive(Debug)]
pub struct Respuesta {
    pub estado: u16,
    pub cabecera: String,
    pub cuerpo: String,
}

const FIN_LINEA: &[u8] = b"\r\n";
const FIN_CABECERA: &[u8] = b"\r\n\r\n";

/// Hace una petición HTTP/1.1 a `127.0.0.1:puerto`.
/// `encabezados` permite agregar `Host`, `Origin`, `X-Vigia-Token`, etc.
/// Si no se da `Host`, se envía el correcto.
pub fn pedir(
    puerto: u16,
    metodo: &str,
    ruta: &str,
    encabezados: &[(&str, &str)],
    cuerpo: &str,
) -> Resultado<Respuesta> {
    let mut s = TcpStream::connect(("127.0.0.1", puerto))?;
    s.set_read_timeout(Some(Duration::from_secs(20)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut peticion = format!("{metodo} {ruta} HTTP/1.1\r\n");
    if !encabezados
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("host"))
    {
        peticion.push_str(&format!("Host: 127.0.0.1:{puerto}\r\n"));
    }
    for (k, v) in encabezados {
        peticion.push_str(&format!("{k}: {v}\r\n"));
    }
    peticion.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{cuerpo}",
        cuerpo.len()
    ));
    s.write_all(peticion.as_bytes())?;
    let mut bytes = Vec::new();
    s.read_to_end(&mut bytes)?;
    let fin_cabecera =
        buscar(&bytes, FIN_CABECERA).ok_or_else(|| error("respuesta HTTP incompleta"))?;
    let cabecera = String::from_utf8_lossy(&bytes[..fin_cabecera]).to_ascii_lowercase();
    let resto = &bytes[fin_cabecera + FIN_CABECERA.len()..];
    let estado = cabecera
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| error("respuesta HTTP sin código"))?;
    // tiny_http puede responder con "Transfer-Encoding: chunked". Se une
    // sobre bytes: un trozo puede cortar un carácter UTF-8 por la mitad.
    let cuerpo = if cabecera.contains("transfer-encoding: chunked") {
        desfragmentar(resto)
    } else {
        resto.to_vec()
    };
    Ok(Respuesta {
        estado,
        cabecera,
        cuerpo: String::from_utf8_lossy(&cuerpo).to_string(),
    })
}

fn buscar(datos: &[u8], patron: &[u8]) -> Option<usize> {
    datos.windows(patron.len()).position(|w| w == patron)
}

/// Une un cuerpo enviado en "trozos" (chunked encoding): cada trozo es el
/// tamaño en hexadecimal, un fin de línea, los bytes y otro fin de línea.
/// Termina con un trozo de tamaño 0.
fn desfragmentar(mut s: &[u8]) -> Vec<u8> {
    let mut salida = Vec::new();
    while let Some(i) = buscar(s, FIN_LINEA) {
        let tam = String::from_utf8_lossy(&s[..i]);
        let Ok(n) = usize::from_str_radix(tam.trim(), 16) else {
            break;
        };
        let resto = &s[i + FIN_LINEA.len()..];
        if n == 0 || resto.len() < n {
            break;
        }
        salida.extend_from_slice(&resto[..n]);
        s = resto[n..].strip_prefix(FIN_LINEA).unwrap_or(&resto[n..]);
    }
    salida
}

#[cfg(test)]
mod pruebas {
    #[test]
    fn desfragmenta() {
        let entrada = b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert_eq!(super::desfragmentar(entrada), b"abcde");
    }
}

//! Red: bytes por adaptador y tabla de conexiones por proceso.

use super::{Resultado, desde_utf16};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

/// Un adaptador de red físico con sus contadores acumulados.
#[derive(Debug, Clone)]
pub struct Adaptador {
    /// Identificador único y estable del adaptador (LUID).
    pub id: u64,
    /// Nombre que ve el usuario, por ejemplo "Wi-Fi" o "Ethernet".
    pub nombre: String,
    /// Nombre del hardware, por ejemplo "Intel(R) Wi-Fi 6 AX201".
    pub descripcion: String,
    pub conectado: bool,
    /// Bytes recibidos y enviados desde que se activó el adaptador.
    pub recibidos: u64,
    pub enviados: u64,
}

/// Lista los adaptadores de red físicos con sus bytes recibidos y enviados.
pub fn adaptadores() -> Resultado<Vec<Adaptador>> {
    let mut tabla: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // GetIfTable2 (iphlpapi): devuelve una tabla con una fila MIB_IF_ROW2
    // por interfaz de red, con contadores de 64 bits (no se desbordan como
    // los de 32 bits de la antigua GetIfTable). Windows reserva la memoria y
    // nosotros debemos liberarla con FreeMibTable.
    unsafe { GetIfTable2(&mut tabla) }.ok()?;
    if tabla.is_null() {
        return Ok(Vec::new());
    }
    // La tabla es un encabezado con `NumEntries` seguido de las filas.
    // `from_raw_parts` crea un slice de Rust sobre esa memoria de Windows.
    let filas = unsafe {
        let n = (*tabla).NumEntries as usize;
        // `Table` está declarada como un arreglo de 1 elemento (estilo C);
        // tomamos la dirección del campo completo para que el puntero
        // "pueda" recorrer las n filas que Windows puso a continuación.
        std::slice::from_raw_parts((&raw const (*tabla).Table).cast::<MIB_IF_ROW2>(), n)
    };
    let mut lista = Vec::new();
    for fila in filas {
        // InterfaceAndOperStatusFlags es un campo de bits: el bit 0 indica
        // "interfaz de hardware" y el bit 1 "interfaz de filtro" (capas que
        // Windows apila sobre un adaptador real y que repetirían sus bytes).
        let bits = fila.InterfaceAndOperStatusFlags._bitfield;
        let hardware = bits & 0b01 != 0;
        let filtro = bits & 0b10 != 0;
        if !hardware || filtro {
            continue;
        }
        lista.push(Adaptador {
            id: unsafe { fila.InterfaceLuid.Value },
            nombre: desde_utf16(&fila.Alias),
            descripcion: desde_utf16(&fila.Description),
            conectado: fila.OperStatus == IfOperStatusUp,
            recibidos: fila.InOctets,
            enviados: fila.OutOctets,
        });
    }
    // FreeMibTable (iphlpapi): libera la memoria que reservó GetIfTable2.
    unsafe { FreeMibTable(tabla.cast()) };
    Ok(lista)
}

// ---------------------------------------------------------------------
// Conexiones: qué proceso tiene qué conexión (sin leer su contenido).
// ---------------------------------------------------------------------

use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};

/// Familias de direcciones (ws2def.h).
const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;

/// Estado TCP "en escucha" (MIB_TCP_STATE_LISTEN).
pub const TCP_ESCUCHA: u32 = 2;

/// Una fila de las tablas de conexiones de Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConexionCruda {
    pub tcp: bool,
    pub local: IpAddr,
    pub puerto_local: u16,
    /// `None` en UDP (no tiene "otro extremo" fijo).
    pub remota: Option<IpAddr>,
    pub puerto_remoto: u16,
    /// Estado TCP (MIB_TCP_STATE: 2 = escucha, 5 = establecida...). 0 en UDP.
    pub estado: u32,
    pub pid: u32,
}

/// Los puertos vienen en "orden de red" (big-endian) en los 16 bits bajos.
fn puerto(p: u32) -> u16 {
    u16::from_be((p & 0xFFFF) as u16)
}

/// Las direcciones IPv4 vienen como 4 bytes en orden de red guardados en un
/// `u32`; `to_ne_bytes` recupera esos bytes tal como están en memoria.
fn ipv4(a: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(a.to_ne_bytes()))
}

/// Pide una tabla a Windows con el patrón habitual de IP Helper: primero
/// se pregunta el tamaño necesario y luego se llama con un búfer de ese
/// tamaño (reintentando si la tabla creció entre medio).
fn pedir_tabla(llamar: impl Fn(*mut c_void, &mut u32) -> u32) -> Resultado<Vec<u32>> {
    let mut tam = 16 * 1024u32;
    for _ in 0..6 {
        // u32 para que el búfer quede alineado a 4 bytes, como las filas.
        let mut bufer = vec![0u32; (tam as usize).div_ceil(4)];
        let mut t = (bufer.len() * 4) as u32;
        let r = llamar(bufer.as_mut_ptr().cast(), &mut t);
        if r == 0 {
            return Ok(bufer);
        }
        if r == ERROR_INSUFFICIENT_BUFFER.0 {
            tam = t + 4096;
            continue;
        }
        return Err(windows::core::Error::from_hresult(
            WIN32_ERROR(r).to_hresult(),
        ));
    }
    Err(windows::core::Error::from_hresult(
        ERROR_INSUFFICIENT_BUFFER.to_hresult(),
    ))
}

/// Interpreta el búfer como "cantidad de filas (u32) + filas de tipo T".
fn filas<T: Copy>(bufer: &[u32]) -> Vec<T> {
    let Some(&n) = bufer.first() else {
        return Vec::new();
    };
    let n = n as usize;
    let disponibles = (bufer.len() - 1) * 4 / std::mem::size_of::<T>();
    // SAFETY: las filas empiezan justo después del contador (todas las
    // estructuras de fila tienen alineación 4) y no leemos más allá del
    // búfer aunque Windows diga lo contrario.
    unsafe { std::slice::from_raw_parts(bufer.as_ptr().add(1).cast::<T>(), n.min(disponibles)) }
        .to_vec()
}

/// Lee todas las conexiones TCP y UDP (IPv4 e IPv6) con el PID dueño.
pub fn conexiones() -> Resultado<Vec<ConexionCruda>> {
    let mut lista = Vec::new();
    // GetExtendedTcpTable (iphlpapi) con TCP_TABLE_OWNER_PID_ALL: tabla de
    // todas las conexiones TCP (en escucha y conectadas) con el PID del
    // proceso dueño. Es la misma fuente que usa `netstat -ano` y no requiere
    // administrador. No da acceso al contenido de las conexiones.
    let t4 = pedir_tabla(|p, t| unsafe {
        GetExtendedTcpTable(Some(p), t, false, AF_INET, TCP_TABLE_OWNER_PID_ALL, 0)
    })?;
    for f in filas::<MIB_TCPROW_OWNER_PID>(&t4) {
        lista.push(ConexionCruda {
            tcp: true,
            local: ipv4(f.dwLocalAddr),
            puerto_local: puerto(f.dwLocalPort),
            remota: Some(ipv4(f.dwRemoteAddr)),
            puerto_remoto: puerto(f.dwRemotePort),
            estado: f.dwState,
            pid: f.dwOwningPid,
        });
    }
    let t6 = pedir_tabla(|p, t| unsafe {
        GetExtendedTcpTable(Some(p), t, false, AF_INET6, TCP_TABLE_OWNER_PID_ALL, 0)
    })?;
    for f in filas::<MIB_TCP6ROW_OWNER_PID>(&t6) {
        lista.push(ConexionCruda {
            tcp: true,
            local: IpAddr::V6(Ipv6Addr::from(f.ucLocalAddr)),
            puerto_local: puerto(f.dwLocalPort),
            remota: Some(IpAddr::V6(Ipv6Addr::from(f.ucRemoteAddr))),
            puerto_remoto: puerto(f.dwRemotePort),
            estado: f.dwState,
            pid: f.dwOwningPid,
        });
    }
    // GetExtendedUdpTable (iphlpapi) con UDP_TABLE_OWNER_PID: puertos UDP
    // abiertos con su PID. UDP no tiene conexiones: solo puertos locales.
    let u4 = pedir_tabla(|p, t| unsafe {
        GetExtendedUdpTable(Some(p), t, false, AF_INET, UDP_TABLE_OWNER_PID, 0)
    })?;
    for f in filas::<MIB_UDPROW_OWNER_PID>(&u4) {
        lista.push(ConexionCruda {
            tcp: false,
            local: ipv4(f.dwLocalAddr),
            puerto_local: puerto(f.dwLocalPort),
            remota: None,
            puerto_remoto: 0,
            estado: 0,
            pid: f.dwOwningPid,
        });
    }
    let u6 = pedir_tabla(|p, t| unsafe {
        GetExtendedUdpTable(Some(p), t, false, AF_INET6, UDP_TABLE_OWNER_PID, 0)
    })?;
    for f in filas::<MIB_UDP6ROW_OWNER_PID>(&u6) {
        lista.push(ConexionCruda {
            tcp: false,
            local: IpAddr::V6(Ipv6Addr::from(f.ucLocalAddr)),
            puerto_local: puerto(f.dwLocalPort),
            remota: None,
            puerto_remoto: 0,
            estado: 0,
            pid: f.dwOwningPid,
        });
    }
    Ok(lista)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn conversiones_de_orden_de_red() {
        // 80 en orden de red = bytes 00 50 → como u16 little-endian 0x5000.
        assert_eq!(puerto(0x5000), 80);
        // 127.0.0.1 en memoria: 7F 00 00 01 → u32 little-endian 0x0100007F.
        assert_eq!(ipv4(0x0100_007F), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn ve_un_puerto_propio_en_escucha() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        let c = conexiones().unwrap();
        let yo = std::process::id();
        assert!(c.iter().any(|x| x.tcp
            && x.estado == TCP_ESCUCHA
            && x.puerto_local == p
            && x.pid == yo
            && x.local == IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }
}

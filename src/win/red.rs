//! Red: bytes por adaptador.

use super::{Resultado, desde_utf16};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
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
        std::slice::from_raw_parts((*tabla).Table.as_ptr(), n)
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

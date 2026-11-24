//! Descripción y empresa que declara un ejecutable (recurso VERSIONINFO).
//!
//! Es la misma información que muestra Explorador → Propiedades → Detalles.
//! Ojo: la escribe quien compiló el programa, así que es lo que el programa
//! *dice* ser, no una garantía (eso sería la firma digital).

use super::{a_utf16, desde_utf16};
use std::ffi::c_void;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::core::PCWSTR;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfoVersion {
    pub descripcion: Option<String>,
    pub empresa: Option<String>,
}

/// Lee el bloque de versión de un archivo. Devuelve vacío si no tiene.
pub fn leer(ruta: &str) -> InfoVersion {
    let ruta_w = a_utf16(ruta);
    // GetFileVersionInfoSizeW (version.dll): tamaño del recurso de versión.
    // Si es 0, el archivo no tiene ese recurso (o no se puede leer).
    let tam = unsafe { GetFileVersionInfoSizeW(PCWSTR(ruta_w.as_ptr()), None) };
    if tam == 0 {
        return InfoVersion::default();
    }
    // Se reserva como `u32` para garantizar alineación a 4 bytes (las
    // estructuras del recurso de versión la necesitan) y se ve como bytes.
    let mut almacen = vec![0u32; (tam as usize).div_ceil(4)];
    // SAFETY: `almacen` tiene al menos `tam` bytes y vive más que `bloque`.
    let bloque: &mut [u8] =
        unsafe { std::slice::from_raw_parts_mut(almacen.as_mut_ptr().cast::<u8>(), tam as usize) };
    // GetFileVersionInfoW (version.dll): copia el recurso completo al búfer.
    if unsafe {
        GetFileVersionInfoW(
            PCWSTR(ruta_w.as_ptr()),
            None,
            tam,
            bloque.as_mut_ptr().cast::<c_void>(),
        )
    }
    .is_err()
    {
        return InfoVersion::default();
    }

    // Los textos están guardados por idioma y página de códigos. Primero
    // preguntamos cuáles hay en "\VarFileInfo\Translation"; si no hay,
    // probamos con inglés de EE. UU. en Unicode (0409/04B0), lo más común.
    let mut traducciones = vec![(0x0409u16, 0x04B0u16)];
    if let Some(bytes) = consultar(bloque, "\\VarFileInfo\\Translation", false) {
        let pares: Vec<(u16, u16)> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| {
                (
                    u16::from_le_bytes([c[0], c[1]]),
                    u16::from_le_bytes([c[2], c[3]]),
                )
            })
            .collect();
        if !pares.is_empty() {
            traducciones = pares;
        }
    }
    let texto = |campo: &str| -> Option<String> {
        traducciones.iter().find_map(|(idioma, pagina)| {
            let clave = format!("\\StringFileInfo\\{idioma:04x}{pagina:04x}\\{campo}");
            let bytes = consultar(bloque, &clave, true)?;
            let utf16: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let s = desde_utf16(&utf16).trim().to_string();
            (!s.is_empty()).then_some(s)
        })
    };
    InfoVersion {
        descripcion: texto("FileDescription"),
        empresa: texto("CompanyName"),
    }
}

/// Busca un valor dentro del bloque de versión y devuelve sus bytes.
/// `es_texto` indica si el largo que da Windows está en caracteres UTF-16.
fn consultar<'a>(bloque: &'a [u8], clave: &str, es_texto: bool) -> Option<&'a [u8]> {
    let clave_w = a_utf16(clave);
    let mut ptr: *mut c_void = std::ptr::null_mut();
    let mut largo = 0u32;
    // VerQueryValueW (version.dll): devuelve un puntero *dentro* del bloque
    // al valor pedido y su largo (en caracteres para textos, en bytes para
    // datos binarios como Translation). No copia nada.
    let ok = unsafe {
        VerQueryValueW(
            bloque.as_ptr().cast::<c_void>(),
            PCWSTR(clave_w.as_ptr()),
            &mut ptr,
            &mut largo,
        )
    };
    if !ok.as_bool() || ptr.is_null() || largo == 0 {
        return None;
    }
    // Convertimos el puntero en una posición dentro de `bloque` y
    // comprobamos que no se salga: así devolvemos un `&[u8]` seguro.
    let inicio = bloque.as_ptr() as usize;
    let p = ptr as usize;
    if p < inicio || p >= inicio + bloque.len() {
        return None;
    }
    let desde = p - inicio;
    let disponible = bloque.len() - desde;
    let bytes = if es_texto {
        largo as usize * 2
    } else {
        largo as usize
    };
    Some(&bloque[desde..desde + bytes.min(disponible)])
}

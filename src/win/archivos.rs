//! Listado de carpetas para el análisis de disco.
//!
//! Se usa `FindFirstFileExW` en lugar de `std::fs::read_dir` + `metadata`
//! porque Windows entrega en **una sola pasada** el nombre, el tamaño y los
//! atributos de cada entrada, sin abrir cada archivo. Con cientos de miles
//! de archivos eso es mucho más rápido, y además deja ver el atributo
//! `REPARSE_POINT`, que marca enlaces simbólicos, uniones (*junctions*) y
//! archivos de OneDrive que no están descargados.

use super::{Resultado, a_utf16, desde_utf16, filetime_a_unix_ms};
use std::ffi::c_void;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FIND_FIRST_EX_LARGE_FETCH, FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW,
    FindNextFileW, WIN32_FIND_DATAW,
};
use windows::core::PCWSTR;

// Atributos de archivo (winnt.h) que nos interesan.
const ATRIBUTO_CARPETA: u32 = 0x10;
const ATRIBUTO_REPARSE: u32 = 0x400;
const ATRIBUTO_SIN_CONEXION: u32 = 0x1000;
const ATRIBUTO_SOLO_EN_LA_NUBE: u32 = 0x0040_0000; // RECALL_ON_DATA_ACCESS

/// Una entrada de carpeta.
#[derive(Debug, Clone)]
pub struct Entrada {
    pub nombre: String,
    pub es_carpeta: bool,
    /// Tamaño lógico del archivo en bytes (0 para carpetas).
    pub tamano: u64,
    /// Enlace simbólico, unión u otro punto de reanálisis: el análisis no
    /// debe entrar en estas carpetas (podría contar dos veces o entrar en
    /// un ciclo infinito).
    pub es_enlace: bool,
    /// Archivo de OneDrive (u otro proveedor) que no está descargado: su
    /// tamaño no ocupa espacio en este disco.
    pub en_la_nube: bool,
    pub modificado_ms: Option<i64>,
}

/// Manija de búsqueda: se cierra con `FindClose`, no con `CloseHandle`.
struct Busqueda(HANDLE);
impl Drop for Busqueda {
    fn drop(&mut self) {
        // FindClose (kernel32): libera la manija de búsqueda.
        unsafe {
            let _ = FindClose(self.0);
        }
    }
}

/// Lista el contenido de una carpeta (sin entrar en subcarpetas).
pub fn listar(carpeta: &str) -> Resultado<Vec<Entrada>> {
    // El prefijo \\?\ desactiva el límite clásico de 260 caracteres en las
    // rutas. Solo vale con rutas absolutas y barras invertidas.
    let base = carpeta.trim_end_matches('\\');
    let patron = if base.starts_with("\\\\") {
        format!("{base}\\*")
    } else {
        format!("\\\\?\\{base}\\*")
    };
    let patron_w = a_utf16(&patron);
    let mut datos = WIN32_FIND_DATAW::default();
    // FindFirstFileExW (kernel32): empieza a recorrer las entradas que
    // coinciden con el patrón "carpeta\*" y llena la primera en `datos`.
    // - FindExInfoBasic: no pide el nombre corto 8.3 (más rápido).
    // - FIND_FIRST_EX_LARGE_FETCH: pide al sistema bloques más grandes.
    let manija = unsafe {
        FindFirstFileExW(
            PCWSTR(patron_w.as_ptr()),
            FindExInfoBasic,
            (&raw mut datos).cast::<c_void>(),
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    let manija = match manija {
        Ok(h) => Busqueda(h),
        // Una carpeta vacía de verdad no existe en NTFS (siempre están "." y
        // ".."), pero en la raíz de algunas unidades puede no haber nada.
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut lista = Vec::new();
    loop {
        let nombre = desde_utf16(&datos.cFileName);
        if nombre != "." && nombre != ".." {
            let atributos = datos.dwFileAttributes;
            let es_carpeta = atributos & ATRIBUTO_CARPETA != 0;
            lista.push(Entrada {
                es_carpeta,
                tamano: if es_carpeta {
                    0
                } else {
                    (u64::from(datos.nFileSizeHigh) << 32) | u64::from(datos.nFileSizeLow)
                },
                es_enlace: atributos & ATRIBUTO_REPARSE != 0,
                en_la_nube: atributos & (ATRIBUTO_SIN_CONEXION | ATRIBUTO_SOLO_EN_LA_NUBE) != 0,
                modificado_ms: filetime_a_unix_ms(datos.ftLastWriteTime),
                nombre,
            });
        }
        // FindNextFileW (kernel32): pasa a la siguiente entrada. Al terminar
        // devuelve el error ERROR_NO_MORE_FILES.
        match unsafe { FindNextFileW(manija.0, &mut datos) } {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(e) => return Err(e),
        }
    }
    Ok(lista)
}

//! Acceso a la API de Windows.
//!
//! Este es el **único** módulo del programa que usa `unsafe`. Cada submódulo
//! envuelve una familia de llamadas a Windows en funciones seguras que
//! devuelven `Result` y tipos de Rust normales (`String`, `u64`, `Vec`...).
//! El resto del programa nunca toca punteros ni manijas (*handles*).
//!
//! Orden sugerido de lectura: `mod.rs` (ayudas comunes) → `sistema.rs` →
//! `proceso.rs` → `version.rs` → `red.rs` → el resto.
#![allow(unsafe_code)]

pub mod archivos;
pub mod notificacion;
pub mod proceso;
pub mod red;
pub mod registro;
pub mod sistema;
pub mod sistema_extra;
pub mod version;

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};

/// Error de este módulo: el código de Windows ya viene con su mensaje
/// legible dentro de `windows::core::Error`.
pub type Resultado<T> = windows::core::Result<T>;

/// Manija de Windows que se cierra sola al salir de alcance (patrón RAII).
///
/// Casi toda función que "abre" algo en Windows (`OpenProcess`,
/// `CreateToolhelp32Snapshot`...) devuelve un `HANDLE` que hay que cerrar con
/// `CloseHandle`. Si lo olvidamos, el proceso va acumulando manijas abiertas.
/// Con `Drop`, Rust lo cierra por nosotros incluso si hay un error a mitad.
pub(crate) struct Manija(pub(crate) HANDLE);

impl Drop for Manija {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // CloseHandle (kernel32): libera la manija. Ignoramos el error:
            // en `drop` no hay nada razonable que hacer si falla.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Convierte un texto de Rust a UTF-16 terminado en cero, que es como
/// las funciones `...W` de Windows esperan recibir las cadenas.
pub(crate) fn a_utf16(texto: &str) -> Vec<u16> {
    texto.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Convierte un búfer UTF-16 (posiblemente terminado en cero) a `String`.
pub(crate) fn desde_utf16(bufer: &[u16]) -> String {
    let fin = bufer.iter().position(|&c| c == 0).unwrap_or(bufer.len());
    String::from_utf16_lossy(&bufer[..fin])
}

/// Un `FILETIME` son dos mitades de 32 bits de un número de 64 bits que
/// cuenta intervalos de 100 nanosegundos.
pub(crate) fn filetime_a_u64(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

/// Diferencia entre la época de Windows (1601-01-01) y la de Unix
/// (1970-01-01), en intervalos de 100 ns.
const EPOCA_WINDOWS_A_UNIX_100NS: u64 = 116_444_736_000_000_000;

/// Convierte una fecha `FILETIME` a milisegundos desde 1970 (hora Unix, UTC).
pub(crate) fn filetime_a_unix_ms(ft: FILETIME) -> Option<i64> {
    let v = filetime_a_u64(ft);
    if v < EPOCA_WINDOWS_A_UNIX_100NS {
        return None;
    }
    i64::try_from((v - EPOCA_WINDOWS_A_UNIX_100NS) / 10_000).ok()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn utf16_ida_y_vuelta() {
        let w = a_utf16("Vigía ñ");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(desde_utf16(&w), "Vigía ñ");
    }

    #[test]
    fn filetime_epoca_unix() {
        let v = EPOCA_WINDOWS_A_UNIX_100NS + 10_000 * 1500;
        let ft = FILETIME {
            dwLowDateTime: v as u32,
            dwHighDateTime: (v >> 32) as u32,
        };
        assert_eq!(filetime_a_unix_ms(ft), Some(1500));
    }
}

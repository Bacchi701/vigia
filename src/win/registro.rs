//! Lectura (solo lectura) del registro y de las carpetas de inicio.
//!
//! Vigía **nunca escribe** en el registro: las claves se abren con
//! `KEY_READ` y solo se enumeran sus valores.

use super::{Resultado, a_utf16, desde_utf16};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_BINARY, REG_EXPAND_SZ, REG_SZ,
    REG_VALUE_TYPE, RegCloseKey, RegEnumValueW, RegOpenKeyExW,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartup, FOLDERID_Startup, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::{GUID, PCWSTR, PWSTR};

/// Rama del registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rama {
    /// HKEY_CURRENT_USER: configuración del usuario actual.
    Usuario,
    /// HKEY_LOCAL_MACHINE: configuración de todo el equipo.
    Equipo,
}

impl Rama {
    fn hkey(self) -> HKEY {
        match self {
            Rama::Usuario => HKEY_CURRENT_USER,
            Rama::Equipo => HKEY_LOCAL_MACHINE,
        }
    }
    pub fn abreviatura(self) -> &'static str {
        match self {
            Rama::Usuario => "HKCU",
            Rama::Equipo => "HKLM",
        }
    }
}

/// Valor de un registro: texto o bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Valor {
    Texto(String),
    Binario(Vec<u8>),
    Otro,
}

/// Clave abierta que se cierra sola.
struct Clave(HKEY);
impl Drop for Clave {
    fn drop(&mut self) {
        // RegCloseKey (advapi32): cierra la clave abierta.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Lee todos los valores de una clave. Si la clave no existe devuelve una
/// lista vacía (es normal: p. ej. `RunOnce` suele no existir).
pub fn valores(rama: Rama, ruta: &str) -> Resultado<Vec<(String, Valor)>> {
    let ruta_w = a_utf16(ruta);
    let mut hkey = HKEY::default();
    // RegOpenKeyExW (advapi32): abre una subclave. Pedimos KEY_READ, que
    // solo permite leer: aunque hubiera un error en el programa, con esta
    // manija es imposible modificar el registro.
    let r = unsafe {
        RegOpenKeyExW(
            rama.hkey(),
            PCWSTR(ruta_w.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
    };
    if r != ERROR_SUCCESS {
        return Ok(Vec::new());
    }
    let clave = Clave(hkey);
    let mut lista = Vec::new();
    let mut nombre = vec![0u16; 16_384]; // largo máximo de un nombre de valor
    let mut datos = vec![0u8; 8 * 1024];
    let mut indice = 0u32;
    loop {
        let mut largo_nombre = nombre.len() as u32;
        let mut largo_datos = datos.len() as u32;
        let mut tipo = REG_VALUE_TYPE::default();
        // RegEnumValueW (advapi32): devuelve el valor número `indice` de la
        // clave: su nombre, su tipo y sus datos. Se llama con 0, 1, 2... hasta
        // que responde ERROR_NO_MORE_ITEMS.
        let r = unsafe {
            RegEnumValueW(
                clave.0,
                indice,
                Some(PWSTR(nombre.as_mut_ptr())),
                &mut largo_nombre,
                None,
                Some(&mut tipo.0),
                Some(datos.as_mut_ptr()),
                Some(&mut largo_datos),
            )
        };
        if r == ERROR_NO_MORE_ITEMS {
            break;
        }
        if r != ERROR_SUCCESS {
            // ERROR_MORE_DATA: un valor con datos enormes; lo agrandamos y
            // reintentamos el mismo índice (una vez, con tope de 1 MB).
            if largo_datos as usize > datos.len() && largo_datos <= 1 << 20 {
                datos.resize(largo_datos as usize, 0);
                continue;
            }
            indice += 1;
            continue;
        }
        let n = String::from_utf16_lossy(&nombre[..largo_nombre as usize]);
        let bytes = &datos[..(largo_datos as usize).min(datos.len())];
        let valor = if tipo == REG_SZ || tipo == REG_EXPAND_SZ {
            let utf16: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            Valor::Texto(desde_utf16(&utf16))
        } else if tipo == REG_BINARY {
            Valor::Binario(bytes.to_vec())
        } else {
            Valor::Otro
        };
        lista.push((n, valor));
        indice += 1;
    }
    Ok(lista)
}

/// Carpetas de inicio: la del usuario y la común a todos los usuarios.
pub fn carpetas_de_inicio() -> Vec<(bool, String)> {
    let mut v = Vec::new();
    for (es_usuario, id) in [(true, &FOLDERID_Startup), (false, &FOLDERID_CommonStartup)] {
        if let Some(r) = carpeta_conocida(id) {
            v.push((es_usuario, r));
        }
    }
    v
}

/// Ruta de una "carpeta conocida" de Windows.
fn carpeta_conocida(id: &GUID) -> Option<String> {
    // SHGetKnownFolderPath (shell32): traduce un identificador de carpeta
    // conocida (como FOLDERID_Startup) a su ruta real, que depende del
    // usuario y del idioma de Windows. Windows reserva la memoria del texto
    // y nosotros debemos liberarla con CoTaskMemFree.
    let p = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    let texto = unsafe { p.to_string() }.ok();
    // CoTaskMemFree (ole32): libera memoria reservada por COM/Shell.
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    texto
}

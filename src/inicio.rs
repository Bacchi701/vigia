//! Programas que arrancan con Windows: claves `Run` del registro y
//! carpetas de inicio. Solo se leen; Vigía no cambia nada.
//!
//! Para avisar de cambios, se guarda la lista de la revisión anterior en
//! la tabla `config` y se compara con la actual.

use crate::win::registro::{self, Rama, Valor};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Claves del registro que se revisan. La columna de "aprobado" indica
/// dónde guarda Windows si el usuario desactivó la entrada desde el
/// Administrador de tareas (pestaña Inicio).
const CLAVES: &[(Rama, &str, Option<&str>)] = &[
    (
        Rama::Usuario,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        Some(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"),
    ),
    (
        Rama::Usuario,
        r"Software\Microsoft\Windows\CurrentVersion\RunOnce",
        None,
    ),
    (
        Rama::Equipo,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        Some(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"),
    ),
    (
        Rama::Equipo,
        r"Software\Microsoft\Windows\CurrentVersion\RunOnce",
        None,
    ),
    (
        Rama::Equipo,
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
        Some(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32"),
    ),
];

/// Una entrada de inicio.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entrada {
    /// Dónde está: "HKCU\...\Run" o "Carpeta de inicio (usuario)".
    pub origen: String,
    pub nombre: String,
    /// Comando completo tal como está guardado.
    pub comando: String,
    /// Ruta del ejecutable extraída del comando (si se pudo).
    pub ejecutable: Option<String>,
    /// ¿Existe ese ejecutable en disco?
    pub existe: Option<bool>,
    /// `Some(false)` si el usuario la desactivó en el Administrador de
    /// tareas; `None` si Windows no guarda esa información para este origen.
    pub habilitada: Option<bool>,
    pub descripcion: Option<String>,
    pub empresa: Option<String>,
}

impl Entrada {
    /// Clave estable para comparar revisiones.
    pub fn clave(&self) -> String {
        format!("{}|{}", self.origen, self.nombre).to_lowercase()
    }
}

/// Reemplaza `%VARIABLE%` por su valor (como hace Windows con REG_EXPAND_SZ).
pub fn expandir_variables(texto: &str) -> String {
    let mut salida = String::new();
    let mut resto = texto;
    while let Some(i) = resto.find('%') {
        salida.push_str(&resto[..i]);
        let tras = &resto[i + 1..];
        match tras.find('%') {
            Some(j) => {
                let nombre = &tras[..j];
                match std::env::var(nombre) {
                    Ok(v) if !nombre.is_empty() => salida.push_str(&v),
                    _ => {
                        salida.push('%');
                        salida.push_str(nombre);
                        salida.push('%');
                    }
                }
                resto = &tras[j + 1..];
            }
            None => {
                salida.push('%');
                resto = tras;
            }
        }
    }
    salida.push_str(resto);
    salida
}

/// Extrae la ruta del ejecutable de un comando como
/// `"C:\Program Files\X\x.exe" --minimizado` o `C:\x\y.exe /s`.
pub fn ejecutable_de(comando: &str) -> Option<String> {
    let c = expandir_variables(comando.trim());
    if let Some(resto) = c.strip_prefix('"') {
        return resto.find('"').map(|i| resto[..i].to_string());
    }
    let minus = c.to_lowercase();
    for ext in [".exe", ".bat", ".cmd", ".com", ".lnk"] {
        if let Some(i) = minus.find(ext) {
            return Some(c[..i + ext.len()].to_string());
        }
    }
    c.split_whitespace().next().map(str::to_string)
}

/// Estado "aprobado" del Administrador de tareas: el primer byte del valor
/// es par (02, 06) si está habilitada e impar (03, 07) si está desactivada.
fn aprobaciones(ruta: &str) -> HashMap<String, bool> {
    let mut m = HashMap::new();
    for rama in [Rama::Usuario, Rama::Equipo] {
        for (nombre, valor) in registro::valores(rama, ruta).unwrap_or_default() {
            if let Valor::Binario(b) = valor
                && let Some(&primero) = b.first()
            {
                m.insert(
                    format!("{}|{}", rama.abreviatura(), nombre.to_lowercase()),
                    primero & 1 == 0,
                );
            }
        }
    }
    m
}

/// Lee todas las entradas de inicio. `version` permite obtener descripción
/// y empresa de cada ejecutable (se pasa desde fuera para reutilizar caché).
pub fn leer(version: impl Fn(&str) -> crate::win::version::InfoVersion) -> Vec<Entrada> {
    let mut lista = Vec::new();
    for (rama, ruta, aprobado) in CLAVES {
        let aprob = aprobado.map(aprobaciones).unwrap_or_default();
        for (nombre, valor) in registro::valores(*rama, ruta).unwrap_or_default() {
            let Valor::Texto(comando) = valor else {
                continue;
            };
            let habilitada = aprobado.map(|_| {
                *aprob
                    .get(&format!("{}|{}", rama.abreviatura(), nombre.to_lowercase()))
                    .unwrap_or(&true)
            });
            lista.push(completar(
                Entrada {
                    origen: format!("{}\\{}", rama.abreviatura(), ruta),
                    nombre,
                    ejecutable: ejecutable_de(&comando),
                    comando,
                    existe: None,
                    habilitada,
                    descripcion: None,
                    empresa: None,
                },
                &version,
            ));
        }
    }
    let aprob_carpeta = aprobaciones(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
    );
    for (es_usuario, carpeta) in registro::carpetas_de_inicio() {
        let Ok(dir) = std::fs::read_dir(&carpeta) else {
            continue;
        };
        for e in dir.flatten() {
            let nombre = e.file_name().to_string_lossy().to_string();
            if nombre.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let ruta = e.path().to_string_lossy().to_string();
            let rama = if es_usuario { "HKCU" } else { "HKLM" };
            lista.push(completar(
                Entrada {
                    origen: if es_usuario {
                        "Carpeta de inicio (usuario)".into()
                    } else {
                        "Carpeta de inicio (todos los usuarios)".into()
                    },
                    habilitada: Some(
                        *aprob_carpeta
                            .get(&format!("{rama}|{}", nombre.to_lowercase()))
                            .unwrap_or(&true),
                    ),
                    nombre,
                    comando: ruta.clone(),
                    ejecutable: Some(ruta),
                    existe: None,
                    descripcion: None,
                    empresa: None,
                },
                &version,
            ));
        }
    }
    lista
}

fn completar(
    mut e: Entrada,
    version: &impl Fn(&str) -> crate::win::version::InfoVersion,
) -> Entrada {
    if let Some(x) = &e.ejecutable {
        e.existe = Some(std::path::Path::new(x).exists());
        if x.to_lowercase().ends_with(".exe") {
            let v = version(x);
            e.descripcion = v.descripcion;
            e.empresa = v.empresa;
        }
    }
    e
}

/// Diferencias entre dos revisiones.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Cambios {
    pub nuevas: Vec<Entrada>,
    pub quitadas: Vec<Entrada>,
}

pub fn comparar(antes: &[Entrada], ahora: &[Entrada]) -> Cambios {
    let a: HashSet<String> = antes.iter().map(Entrada::clave).collect();
    let b: HashSet<String> = ahora.iter().map(Entrada::clave).collect();
    Cambios {
        nuevas: ahora
            .iter()
            .filter(|e| !a.contains(&e.clave()))
            .cloned()
            .collect(),
        quitadas: antes
            .iter()
            .filter(|e| !b.contains(&e.clave()))
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn e(nombre: &str) -> Entrada {
        Entrada {
            origen: "HKCU\\Run".into(),
            nombre: nombre.into(),
            comando: "x.exe".into(),
            ejecutable: None,
            existe: None,
            habilitada: None,
            descripcion: None,
            empresa: None,
        }
    }

    #[test]
    fn extrae_ejecutables() {
        assert_eq!(
            ejecutable_de(r#""C:\Program Files\App\app.exe" --oculto"#).as_deref(),
            Some(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(
            ejecutable_de(r"C:\Herramientas\x.EXE /s").as_deref(),
            Some(r"C:\Herramientas\x.EXE")
        );
        let windir = std::env::var("windir").unwrap();
        assert_eq!(
            ejecutable_de(r"%windir%\system32\cmd.exe /c algo"),
            Some(format!(r"{windir}\system32\cmd.exe"))
        );
    }

    #[test]
    fn expande_variables_y_deja_las_desconocidas() {
        assert_eq!(expandir_variables("100%"), "100%");
        assert_eq!(
            expandir_variables("%NO_EXISTE_123%\\x"),
            "%NO_EXISTE_123%\\x"
        );
    }

    #[test]
    fn compara_revisiones() {
        let c = comparar(&[e("a"), e("b")], &[e("B"), e("c")]);
        assert_eq!(c.nuevas, vec![e("c")]);
        assert_eq!(c.quitadas, vec![e("a")]);
    }

    #[test]
    fn lee_el_inicio_real_sin_fallar() {
        // No sabemos qué hay en este equipo, pero leer no debe fallar y
        // toda entrada debe tener origen y nombre.
        let l = leer(crate::win::version::leer);
        assert!(
            l.iter()
                .all(|x| !x.origen.is_empty() && !x.nombre.is_empty())
        );
    }
}

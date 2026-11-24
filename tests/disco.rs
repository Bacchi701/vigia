//! Análisis de disco sobre un árbol de prueba generado dentro de `target/`
//! y sobre la carpeta del proyecto. Nunca sobre otras carpetas.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use vigia::disco::{self, Progreso};

fn escribir(ruta: &Path, bytes: usize) {
    std::fs::create_dir_all(ruta.parent().unwrap()).unwrap();
    std::fs::write(ruta, vec![b'x'; bytes]).unwrap();
}

/// Crea (o deja como estaba) el árbol de prueba. Devuelve su raíz.
fn arbol(nombre: &str) -> PathBuf {
    let raiz = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(nombre);
    escribir(&raiz.join("raiz.txt"), 10);
    escribir(&raiz.join("a").join("uno.dat"), 1000);
    escribir(&raiz.join("a").join("dos.dat"), 1000);
    escribir(&raiz.join("a").join("tres.txt"), 1000);
    escribir(&raiz.join("a").join("b").join("registro.log"), 5000);
    escribir(&raiz.join("c").join("datos.bin"), 20000);
    // Unión (junction) que apunta a la raíz: un ciclo infinito si el
    // análisis entrara en ella. `mklink /J` no requiere administrador.
    let vuelta = raiz.join("c").join("vuelta");
    if std::fs::symlink_metadata(&vuelta).is_err() {
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&vuelta)
            .arg(&raiz)
            .output()
            .unwrap();
        assert!(ok.status.success(), "{ok:?}");
    }
    // Archivo que la segunda parte de la prueba agrega.
    let _ = std::fs::remove_file(raiz.join("a").join("b").join("nuevo.log"));
    raiz
}

fn analizar(raiz: &str) -> disco::Analisis {
    disco::analizar(
        raiz,
        || 0,
        &AtomicBool::new(false),
        &Mutex::new(Progreso::default()),
    )
}

#[test]
fn analiza_el_arbol_de_prueba_salta_la_union_y_compara() {
    let raiz = arbol("arbol_disco");
    let raiz_txt = disco::normalizar_raiz(&raiz.to_string_lossy()).unwrap();
    let a = analizar(&raiz_txt);
    assert!(a.completo);
    assert_eq!(a.total, 28_010);
    assert_eq!(a.archivos, 6);
    assert_eq!(a.enlaces_omitidos, 1);
    assert_eq!(a.errores, 0);
    // Árbol: c (20000) primero, luego a (8000) con b (5000) adentro.
    assert_eq!(a.arbol.tamano, 28_010);
    assert_eq!(a.arbol.hijos[0].nombre, "c");
    assert_eq!(a.arbol.hijos[1].nombre, "a");
    assert_eq!(a.arbol.hijos[1].tamano, 8000);
    assert_eq!(a.arbol.hijos[1].hijos[0].nombre, "b");
    // Archivos grandes y tipos.
    assert!(a.grandes[0].ruta.ends_with("c\\datos.bin"));
    assert_eq!(a.grandes[0].tamano, 20_000);
    let tipo = |e: &str| a.tipos.iter().find(|t| t.extension == e).unwrap();
    assert_eq!(tipo("bin").bytes, 20_000);
    assert_eq!(tipo("dat").archivos, 2);
    assert_eq!(tipo("txt").bytes, 1010);

    // Guardar, agregar un archivo y comparar.
    let conn = vigia::base::en_memoria().unwrap();
    disco::guardar(&conn, &a).unwrap();
    escribir(&raiz.join("a").join("b").join("nuevo.log"), 7000);
    let mut b = analizar(&raiz_txt);
    b.id = disco::guardar(&conn, &b).unwrap();
    let previo = disco::anterior(&conn, &b)
        .unwrap()
        .expect("análisis anterior");
    let cambios = disco::comparar(&previo, &b);
    let crecio = cambios.iter().find(|c| c.ruta.ends_with("a\\b")).unwrap();
    assert_eq!(crecio.diferencia, 7000);
    assert!(cambios.iter().all(|c| !c.ruta.ends_with("\\c")));
    // Solo se pueden abrir rutas del análisis.
    assert!(disco::contiene_ruta(&b, &b.grandes[0].ruta));
    assert!(!disco::contiene_ruta(&b, "C:\\Windows"));
    let _ = std::fs::remove_file(raiz.join("a").join("b").join("nuevo.log"));
}

#[test]
fn se_puede_cancelar() {
    let raiz = arbol("arbol_disco_cancelar");
    let raiz_txt = disco::normalizar_raiz(&raiz.to_string_lossy()).unwrap();
    let a = disco::analizar(
        &raiz_txt,
        || 0,
        &AtomicBool::new(true),
        &Mutex::new(Progreso::default()),
    );
    assert!(!a.completo);
    assert_eq!(a.archivos, 0);
}

#[test]
fn analiza_la_carpeta_del_proyecto() {
    let raiz = disco::normalizar_raiz(env!("CARGO_MANIFEST_DIR")).unwrap();
    let a = analizar(&raiz);
    assert!(a.completo);
    assert!(a.archivos > 10);
    assert!(a.tipos.iter().any(|t| t.extension == "rs"));
    assert!(a.tipos.iter().any(|t| t.extension == "toml"));
    // La carpeta target pesa más que src.
    let target = a.arbol.hijos.iter().find(|h| h.nombre == "target").unwrap();
    assert!(target.tamano > 1_000_000);
}

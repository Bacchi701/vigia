//! Historial con muestras reales y base en disco (dentro de `target/`).

use vigia::colector::Colector;
use vigia::historial::{self, Historial, MINUTO_MS, Orden};

#[test]
fn muestras_reales_se_guardan_y_se_consultan_tras_reabrir() {
    // CARGO_TARGET_TMPDIR es una carpeta dentro de target/ para pruebas.
    let ruta = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("historial_prueba.db");
    let _ = std::fs::remove_file(&ruta);
    let _ = std::fs::remove_file(ruta.with_extension("db-wal"));
    let _ = std::fs::remove_file(ruta.with_extension("db-shm"));

    let mut colector = Colector::new();
    let mut h = Historial::new(100);
    // Hora simulada: tres muestras reales en el minuto T y una en T+1 que
    // cierra el minuto T.
    let t = historial::minuto_de(1_790_000_000_000);
    {
        let mut conn = vigia::base::abrir(&ruta).unwrap();
        for (i, ts) in [t, t + 2000, t + 4000, t + MINUTO_MS]
            .into_iter()
            .enumerate()
        {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            let m = colector.muestrear(ts).unwrap();
            if let Some(r) = h.registrar(&m) {
                historial::guardar_minuto(&mut conn, &r).unwrap();
            }
        }
    }
    let conn = vigia::base::abrir(&ruta).unwrap();
    let m = historial::momento(&conn, t)
        .unwrap()
        .expect("minuto guardado");
    assert_eq!(m.sistema.muestras, 3);
    assert!(!m.apps.is_empty());
    let top = historial::top_apps(&conn, t, t + MINUTO_MS, Orden::Memoria, 5).unwrap();
    assert!(!top.is_empty());
    assert!(top[0].mem_prom > 0);
}

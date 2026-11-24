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
    let m = historial::momento(&conn, t, 0)
        .unwrap()
        .expect("minuto guardado");
    assert_eq!(m.sistema.muestras, 3);
    assert!(!m.apps.is_empty());
    let top = historial::top_apps(&conn, t, t + MINUTO_MS, Orden::Memoria, 5).unwrap();
    assert!(!top.is_empty());
    assert!(top[0].mem_prom > 0);
}

#[test]
fn la_retencion_achica_el_archivo_de_verdad() {
    use vigia::historial::{Historial, MINUTO_MS};
    let ruta = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("retencion_prueba.db");
    for ext in ["db", "db-wal", "db-shm"] {
        let _ = std::fs::remove_file(ruta.with_extension(ext));
    }
    let mut conn = vigia::base::abrir(&ruta).unwrap();
    let mut h = Historial::new(10);
    let inicio = historial::minuto_de(1_790_000_000_000);
    let mut colector_falso = Vec::new();
    for i in 0..40 {
        colector_falso.push(format!("app{i}.exe"));
    }
    // 4 horas de minutos con 40 apps distintas por hora.
    let mut ts = inicio;
    while ts < inicio + 240 * MINUTO_MS {
        let procesos = colector_falso
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let mut p = vigia::colector::Proceso {
                    pid: i as u32 + 1,
                    ppid: 0,
                    nombre: format!("{}_{}", n, ts / 3_600_000),
                    ruta: None,
                    cpu: 1.0,
                    memoria: 1 << 20,
                    memoria_fisica: 1 << 20,
                    memoria_comprometida: 1 << 20,
                    io_lectura_bps: 0.0,
                    io_escritura_bps: 0.0,
                    io_lectura_total: 0,
                    io_escritura_total: 0,
                    inicio_ms: Some(1),
                    descripcion: None,
                    empresa: None,
                    hilos: 1,
                    manijas: 1,
                    acceso_limitado: false,
                };
                p.cpu = 1.0;
                p
            })
            .collect();
        let m = vigia::colector::Muestra {
            ts,
            procesos,
            ..Default::default()
        };
        if let Some(r) = h.registrar(&m) {
            historial::guardar_minuto(&mut conn, &r).unwrap();
        }
        ts += 10_000;
    }
    let paginas = |c: &rusqlite::Connection| -> i64 {
        c.query_row("PRAGMA page_count", [], |f| f.get(0)).unwrap()
    };
    let antes = paginas(&conn);
    let usado = vigia::base::bytes_usados(&conn).unwrap();
    historial::aplicar_retencion(&conn, ts, 30, usado / 3).unwrap();
    let despues = paginas(&conn);
    assert!(
        despues * 2 < antes,
        "el archivo no se achicó: {antes} → {despues} páginas"
    );
}

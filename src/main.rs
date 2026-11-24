//! Punto de entrada de `vigia.exe`: interpreta los argumentos.
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let codigo = match args.first().map(String::as_str) {
        Some("--prueba-cpu") => prueba_cpu(&args),
        Some("--prueba-memoria") => prueba_memoria(&args),
        Some("--muestra") => muestra_por_consola(),
        _ => muestra_por_consola(),
    };
    std::process::exit(codigo);
}

/// Lee el argumento número `i` como entero, con un valor por omisión.
fn arg_num(args: &[String], i: usize, omision: u64) -> u64 {
    args.get(i).and_then(|s| s.parse().ok()).unwrap_or(omision)
}

/// Modo de prueba: ocupa un núcleo de CPU durante N segundos y termina solo.
/// Lo usan las pruebas automáticas para comprobar que Vigía lo detecta.
fn prueba_cpu(args: &[String]) -> i32 {
    let segundos = arg_num(args, 1, 5).min(120);
    let fin = Instant::now() + Duration::from_secs(segundos);
    let mut x: u64 = 1;
    while Instant::now() < fin {
        for _ in 0..100_000 {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
        }
        std::hint::black_box(x);
    }
    0
}

/// Modo de prueba: reserva y toca N MB de memoria durante S segundos.
fn prueba_memoria(args: &[String]) -> i32 {
    let mb = arg_num(args, 1, 200).min(2048) as usize;
    let segundos = arg_num(args, 2, 5).min(120);
    // Escribir en cada página obliga a Windows a darle memoria física real.
    let bloque = vec![1u8; mb * 1024 * 1024];
    std::hint::black_box(&bloque);
    std::thread::sleep(Duration::from_secs(segundos));
    std::hint::black_box(bloque.len());
    0
}

fn muestra_por_consola() -> i32 {
    let mut c = vigia::colector::Colector::new();
    let _ = c.muestrear(0);
    std::thread::sleep(Duration::from_secs(1));
    match c.muestrear(0) {
        Ok(m) => {
            println!(
                "CPU {:.1} %  Mem {} / {} MB  Red rx {:.0} B/s tx {:.0} B/s  Procesos {}",
                m.sistema.cpu,
                m.sistema.mem_usada / 1_048_576,
                m.sistema.mem_total / 1_048_576,
                m.sistema.red_rx_bps,
                m.sistema.red_tx_bps,
                m.sistema.n_procesos
            );
            for u in &m.unidades {
                println!(
                    "  {} [{}] {} libre de {} GB",
                    u.raiz,
                    u.etiqueta,
                    u.libre / 1_073_741_824,
                    u.total / 1_073_741_824
                );
            }
            for a in &m.adaptadores {
                println!(
                    "  {} ({}) conectado={} rx={} tx={}",
                    a.nombre, a.descripcion, a.conectado, a.recibidos, a.enviados
                );
            }
            let mut p = m.procesos.clone();
            p.sort_by_key(|x| std::cmp::Reverse(x.memoria));
            for x in p.iter().take(8) {
                println!(
                    "  {:>6} {:<25} {:>5.1}% {:>6} MB  {:?} | {:?} | {:?}",
                    x.pid,
                    x.nombre,
                    x.cpu,
                    x.memoria / 1_048_576,
                    x.descripcion,
                    x.empresa,
                    x.ruta
                );
            }
            let limitados = m.procesos.iter().filter(|p| p.acceso_limitado).count();
            println!("  procesos con acceso limitado: {limitados}");
            0
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

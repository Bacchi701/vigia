//! Punto de entrada de `vigia.exe`: interpreta los argumentos.
//!
//! Uso:
//! - `vigia` → inicia Vigía y abre el navegador.
//! - `vigia --sin-navegador` → inicia sin abrir el navegador.
//! - `vigia --puerto N` → usa el puerto N (por omisión 47613).
//! - `vigia --datos CARPETA` → guarda los datos en otra carpeta (pruebas).
//! - `vigia --detener` → pide a la instancia en marcha que se cierre.
//! - `vigia --muestra` → imprime una muestra en la consola y termina.
//! - `vigia --prueba-cpu S` / `--prueba-memoria MB S` → modos de prueba.
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use vigia::app::{self, ArchivoEstado, Config};
use vigia::cliente;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let codigo = match args.first().map(String::as_str) {
        Some("--prueba-cpu") => prueba_cpu(&args),
        Some("--prueba-memoria") => prueba_memoria(&args),
        Some("--muestra") => muestra_por_consola(),
        Some("--detener") => detener(&args),
        Some("--ayuda") | Some("-h") | Some("--help") => {
            println!("{AYUDA}");
            0
        }
        _ => iniciar(&args),
    };
    std::process::exit(codigo);
}

const AYUDA: &str = "Vigía: monitor local de Windows (solo observa).
  vigia                  inicia y abre el navegador
  vigia --sin-navegador  inicia sin abrir el navegador
  vigia --puerto N       usa el puerto N en 127.0.0.1
  vigia --detener        cierra la instancia en marcha
  vigia --datos CARPETA  usa otra carpeta de datos (por omisión, la carpeta «datos»)
  vigia --muestra        imprime una muestra y termina";

/// Carpeta del proyecto: si el ejecutable está en `...\target\release`,
/// es la carpeta que contiene `target`; si no, la carpeta del ejecutable.
fn carpeta_proyecto() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(PathBuf::from).unwrap_or_default();
    let nombre = |p: &std::path::Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    if matches!(nombre(&dir).as_str(), "release" | "debug")
        && let Some(target) = dir.parent()
        && nombre(target) == "target"
        && let Some(raiz) = target.parent()
    {
        return raiz.to_path_buf();
    }
    dir
}

/// Valor que sigue a la opción `nombre` en la línea de comandos.
fn opcion<'a>(args: &'a [String], nombre: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == nombre)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Carpeta de datos: `--datos CARPETA` o `datos` dentro del proyecto.
fn carpeta_datos(args: &[String]) -> PathBuf {
    opcion(args, "--datos")
        .map(PathBuf::from)
        .unwrap_or_else(|| carpeta_proyecto().join("datos"))
}

/// Lee el archivo de estado de una instancia en marcha, si responde.
fn instancia_en_marcha(datos: &Path) -> Option<ArchivoEstado> {
    let texto = std::fs::read_to_string(app::ruta_archivo_estado(datos)).ok()?;
    let e: ArchivoEstado = serde_json::from_str(&texto).ok()?;
    let r = cliente::pedir(
        e.puerto,
        "GET",
        "/api/estado",
        &[("X-Vigia-Token", &e.token)],
        "",
    )
    .ok()?;
    (r.estado == 200).then_some(e)
}

fn iniciar(args: &[String]) -> i32 {
    let abrir_navegador = !args.iter().any(|a| a == "--sin-navegador");
    let puerto = opcion(args, "--puerto")
        .and_then(|p| p.parse().ok())
        .unwrap_or(app::PUERTO_POR_OMISION);
    let datos = carpeta_datos(args);

    vigia::win::sistema_extra::sin_dialogos_de_error();

    // Una sola instancia por carpeta de datos. El mutex con nombre es
    // atómico: aunque se abran dos a la vez, solo una lo obtiene. La otra
    // espera a que la primera esté lista y abre su página.
    let bloqueo = match vigia::win::sistema_extra::instancia_unica(&nombre_bloqueo(&datos)) {
        Ok(Some(b)) => b,
        Ok(None) => {
            let limite = Instant::now() + Duration::from_secs(10);
            let mut otra = instancia_en_marcha(&datos);
            while otra.is_none() && Instant::now() < limite {
                std::thread::sleep(Duration::from_millis(200));
                otra = instancia_en_marcha(&datos);
            }
            match otra {
                Some(e) => {
                    println!("Vigía ya está en marcha en el puerto {}.", e.puerto);
                    if abrir_navegador {
                        let url = format!("http://127.0.0.1:{}/#token={}", e.puerto, e.token);
                        let _ = vigia::win::sistema_extra::abrir(&url);
                    }
                }
                None => println!("Vigía ya está en marcha, pero todavía no responde."),
            }
            return 0;
        }
        Err(e) => {
            eprintln!("No se pudo comprobar si Vigía ya está en marcha: {e}");
            return 1;
        }
    };

    if let Err(e) = vigia::win::sistema_extra::escuchar_cierre_consola() {
        eprintln!("Aviso: no se pudo registrar el cierre de consola: {e}");
    }
    let config = Config {
        dir_datos: Some(datos.clone()),
        puerto,
        ..Config::default()
    };
    let app = match app::iniciar(config) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("No se pudo iniciar Vigía: {e}");
            return 1;
        }
    };
    println!(
        "Vigía está funcionando en http://127.0.0.1:{}/",
        app.puerto()
    );
    println!("Datos en {}", datos.display());
    println!("Para cerrarlo usa detener_vigia.bat (o cierra esta ventana).");
    if abrir_navegador && let Err(e) = vigia::win::sistema_extra::abrir(&app.url()) {
        eprintln!(
            "No se pudo abrir el navegador: {e}. Abre a mano: {}",
            app.url()
        );
    }
    app.esperar();
    drop(bloqueo);
    println!("Vigía se cerró.");
    0
}

/// Nombre del mutex de instancia única: depende de la carpeta de datos,
/// así una instancia de prueba con `--datos` no choca con la real.
fn nombre_bloqueo(datos: &Path) -> String {
    // FNV-1a: un hash simple y estable para convertir la ruta en un número.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in datos.to_string_lossy().to_lowercase().bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("Local\\Vigia-{h:016x}")
}

fn detener(args: &[String]) -> i32 {
    let datos = carpeta_datos(args);
    let Some(e) = instancia_en_marcha(&datos) else {
        println!("Vigía no está en marcha.");
        return 0;
    };
    match cliente::pedir(
        e.puerto,
        "POST",
        "/api/apagar",
        &[("X-Vigia-Token", &e.token)],
        "",
    ) {
        Ok(r) if r.estado == 200 => {
            // Esperar a que borre su archivo de estado (señal de que guardó
            // todo y terminó), hasta 10 s.
            let archivo = app::ruta_archivo_estado(&datos);
            let limite = Instant::now() + Duration::from_secs(10);
            while archivo.exists() && Instant::now() < limite {
                std::thread::sleep(Duration::from_millis(100));
            }
            if archivo.exists() {
                eprintln!("Vigía recibió la orden, pero no confirmó el cierre en 10 s.");
                return 1;
            }
            println!("Vigía se cerró.");
            0
        }
        Ok(r) => {
            eprintln!("Vigía respondió {}: {}", r.estado, r.cuerpo);
            1
        }
        Err(err) => {
            eprintln!("No se pudo contactar a Vigía: {err}");
            1
        }
    }
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
    match c.muestrear(app::ahora_ms()) {
        Ok(m) => {
            println!(
                "CPU {:.1} %  Memoria {} / {} MB  Red ↓ {:.0} B/s ↑ {:.0} B/s  Procesos {}",
                m.sistema.cpu,
                m.sistema.mem_usada / 1_048_576,
                m.sistema.mem_total / 1_048_576,
                m.sistema.red_rx_bps,
                m.sistema.red_tx_bps,
                m.sistema.n_procesos
            );
            for u in &m.unidades {
                println!(
                    "  {} [{}] {} GB libres de {} GB",
                    u.raiz,
                    u.etiqueta,
                    u.libre / 1_073_741_824,
                    u.total / 1_073_741_824
                );
            }
            let mut p = m.procesos.clone();
            p.sort_by_key(|x| std::cmp::Reverse(x.memoria));
            for x in p.iter().take(10) {
                println!(
                    "  {:>6} {:<28} {:>5.1} % {:>7} MB  {}",
                    x.pid,
                    x.nombre,
                    x.cpu,
                    x.memoria / 1_048_576,
                    x.descripcion.as_deref().unwrap_or("")
                );
            }
            let sin_ruta = m.procesos.iter().filter(|p| p.acceso_limitado).count();
            println!("  procesos sin ruta legible: {sin_ruta}");
            0
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

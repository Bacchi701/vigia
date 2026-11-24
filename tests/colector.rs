//! Pruebas del colector contra el Windows real.

use std::process::{Child, Command};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use vigia::colector::Colector;

/// Proceso de prueba que se cierra siempre al terminar la prueba, aunque
/// la prueba falle (por eso está en `Drop`).
struct ProcesoPrueba(Child);

impl Drop for ProcesoPrueba {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn lanzar(args: &[&str]) -> ProcesoPrueba {
    ProcesoPrueba(
        Command::new(env!("CARGO_BIN_EXE_vigia"))
            .args(args)
            .spawn()
            .expect("no se pudo lanzar el proceso de prueba"),
    )
}

fn ahora_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn el_propio_proceso_tiene_valores_coherentes() {
    let mut c = Colector::new();
    c.muestrear(ahora_ms()).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let m = c.muestrear(ahora_ms()).unwrap();

    let yo = std::process::id();
    let p = m
        .procesos
        .iter()
        .find(|p| p.pid == yo)
        .expect("no me encontré");
    let exe = std::env::current_exe().unwrap();
    assert_eq!(
        p.ruta.as_deref().map(str::to_lowercase),
        Some(exe.to_string_lossy().to_lowercase())
    );
    assert!(
        p.memoria > 0 && p.memoria < 2_000_000_000,
        "memoria {}",
        p.memoria
    );
    assert!(p.memoria_fisica >= p.memoria);
    assert!((0.0..=100.0).contains(&p.cpu));
    let inicio = p.inicio_ms.unwrap();
    assert!(inicio <= ahora_ms() && inicio > ahora_ms() - 3_600_000);
    assert!(p.hilos >= 1);

    // Totales del sistema.
    assert!(m.sistema.mem_total > 0 && m.sistema.mem_usada <= m.sistema.mem_total);
    assert!((0.0..=100.0).contains(&m.sistema.cpu));
    assert!(m.sistema.n_procesos > 10);
    assert!(m.unidades.iter().any(|u| u.raiz.starts_with("C:")));
    assert!(m.unidades.iter().all(|u| u.libre <= u.total));
}

#[test]
fn detecta_un_proceso_que_consume_cpu() {
    let hijo = lanzar(&["--prueba-cpu", "8"]);
    let pid = hijo.0.id();
    let mut c = Colector::new();
    std::thread::sleep(Duration::from_millis(500));
    c.muestrear(ahora_ms()).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let m = c.muestrear(ahora_ms()).unwrap();
    let p = m
        .procesos
        .iter()
        .find(|p| p.pid == pid)
        .expect("no aparece");
    // Un hilo ocupado = 100 % de un núcleo = 100 / n_cpu % del equipo.
    let un_nucleo = 100.0 / f64::from(m.sistema.n_cpu);
    assert!(
        p.cpu > un_nucleo * 0.5,
        "CPU medida {:.2} %, se esperaba cerca de {:.2} %",
        p.cpu,
        un_nucleo
    );
    // Y debería estar entre los que más CPU usan.
    let mut orden: Vec<_> = m.procesos.iter().collect();
    orden.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    assert!(orden.iter().take(10).any(|x| x.pid == pid));
}

#[test]
fn detecta_un_proceso_que_consume_memoria() {
    let hijo = lanzar(&["--prueba-memoria", "300", "8"]);
    let pid = hijo.0.id();
    let mut c = Colector::new();
    // Esperar a que reserve y toque la memoria.
    let mut memoria = 0;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(250));
        let m = c.muestrear(ahora_ms()).unwrap();
        if let Some(p) = m.procesos.iter().find(|p| p.pid == pid) {
            memoria = p.memoria;
            if memoria > 250 * 1024 * 1024 {
                break;
            }
        }
    }
    assert!(memoria > 250 * 1024 * 1024, "memoria medida {memoria}");
}

#[test]
fn procesos_del_sistema_no_rompen_nada() {
    let mut c = Colector::new();
    let m = c.muestrear(ahora_ms()).unwrap();
    let system = m.procesos.iter().find(|p| p.pid == 4).expect("System");
    assert_eq!(system.ruta, None);
    assert!(system.acceso_limitado);
    // Casi todos los procesos deberían tener ruta gracias a la alternativa
    // sin abrir el proceso.
    let sin_ruta = m.procesos.iter().filter(|p| p.ruta.is_none()).count();
    assert!(sin_ruta * 10 < m.procesos.len(), "{sin_ruta} sin ruta");
}

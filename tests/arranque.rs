//! Arranque real de `vigia.exe` como lo hace `iniciar_vigia.bat`, pero sin
//! navegador y con una carpeta de datos de prueba dentro de `target/`.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use vigia::app::ArchivoEstado;

struct Hijo(Child);
impl Drop for Hijo {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn exe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vigia"))
}

#[test]
fn inicia_responde_y_se_detiene_con_detener() {
    let datos = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("arranque_datos");
    let estado = datos.join("vigia_estado.json");
    let _ = std::fs::remove_file(&estado);
    let datos_txt = datos.to_string_lossy().to_string();

    let mut hijo = Hijo(
        exe()
            .args(["--sin-navegador", "--puerto", "0", "--datos", &datos_txt])
            .spawn()
            .unwrap(),
    );

    // Espera a que escriba su archivo de estado.
    let limite = Instant::now() + Duration::from_secs(15);
    let e: ArchivoEstado = loop {
        if let Ok(t) = std::fs::read_to_string(&estado)
            && let Ok(e) = serde_json::from_str(&t)
        {
            break e;
        }
        assert!(Instant::now() < limite, "no escribió vigia_estado.json");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(e.pid, hijo.0.id());
    let r = vigia::cliente::pedir(e.puerto, "GET", "/", &[], "").unwrap();
    assert_eq!(r.estado, 200);

    // Un segundo arranque no crea otra instancia: detecta la primera y sale.
    let segundo = exe()
        .args(["--sin-navegador", "--datos", &datos_txt])
        .output()
        .unwrap();
    assert!(segundo.status.success());
    assert!(String::from_utf8_lossy(&segundo.stdout).contains("ya está en marcha"));

    // --detener la cierra ordenadamente y borra el archivo de estado.
    let salida = exe()
        .args(["--detener", "--datos", &datos_txt])
        .output()
        .unwrap();
    assert!(salida.status.success(), "{salida:?}");
    let limite = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(codigo) = hijo.0.try_wait().unwrap() {
            assert!(codigo.success());
            break;
        }
        assert!(Instant::now() < limite, "no se cerró");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!estado.exists());
    assert!(datos.join("vigia.db").exists());
}

#[test]
fn dos_arranques_simultaneos_dejan_una_sola_instancia() {
    let datos = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("arranque_doble");
    let _ = std::fs::remove_file(datos.join("vigia_estado.json"));
    let d = datos.to_string_lossy().to_string();
    let args = ["--sin-navegador", "--puerto", "0", "--datos", d.as_str()];
    let mut a = Hijo(exe().args(args).spawn().unwrap());
    let mut b = Hijo(exe().args(args).spawn().unwrap());

    // La que no obtuvo el bloqueo debe terminar sola (con código 0).
    let limite = Instant::now() + Duration::from_secs(15);
    let (viva, terminada) = loop {
        let fa = a.0.try_wait().unwrap();
        let fb = b.0.try_wait().unwrap();
        match (fa, fb) {
            (Some(c), None) => break (&mut b, c),
            (None, Some(c)) => break (&mut a, c),
            (Some(_), Some(_)) => panic!("terminaron las dos"),
            (None, None) => {}
        }
        assert!(Instant::now() < limite, "siguen las dos en marcha");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(terminada.success());
    // La que quedó se detiene con --detener.
    let salida = exe().args(["--detener", "--datos", &d]).output().unwrap();
    assert!(salida.status.success(), "{salida:?}");
    let limite = Instant::now() + Duration::from_secs(10);
    while viva.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < limite, "no se cerró");
        std::thread::sleep(Duration::from_millis(100));
    }
}

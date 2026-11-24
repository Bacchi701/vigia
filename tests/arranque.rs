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

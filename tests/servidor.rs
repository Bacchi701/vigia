//! Pruebas del servidor: se levanta Vigía dentro de la prueba (base en
//! memoria, puerto libre cualquiera) y se le hacen peticiones HTTP reales.

use std::time::{Duration, Instant};
use vigia::app::{self, App, Config};
use vigia::cliente::{Respuesta, pedir};

/// Vigía en marcha que se detiene siempre al terminar la prueba.
struct Prueba(Option<App>);

impl Drop for Prueba {
    fn drop(&mut self) {
        if let Some(a) = self.0.take() {
            a.detener();
        }
    }
}

impl Prueba {
    fn nueva() -> Prueba {
        let config = Config {
            dir_datos: None,
            puerto: 0,
            intervalo: Duration::from_millis(250),
            ..Config::default()
        };
        Prueba(Some(app::iniciar(config).unwrap()))
    }
    fn app(&self) -> &App {
        self.0.as_ref().unwrap()
    }
    fn get(&self, ruta: &str) -> Respuesta {
        let token = self.app().token().to_string();
        pedir(
            self.app().puerto(),
            "GET",
            ruta,
            &[("X-Vigia-Token", &token)],
            "",
        )
        .unwrap()
    }
    fn json(&self, ruta: &str) -> serde_json::Value {
        let r = self.get(ruta);
        assert_eq!(r.estado, 200, "{ruta}: {}", r.cuerpo);
        serde_json::from_str(&r.cuerpo).unwrap()
    }
    /// Espera a que el colector haya tomado al menos `n` muestras.
    fn esperar_muestras(&self, n: usize) -> serde_json::Value {
        let limite = Instant::now() + Duration::from_secs(10);
        loop {
            let v = self.json("/api/ahora");
            if !v["muestra"].is_null() && v["vivo"].as_array().unwrap().len() >= n {
                return v;
            }
            assert!(Instant::now() < limite, "el colector no produjo muestras");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
fn la_pagina_carga_con_cabeceras_de_seguridad() {
    let p = Prueba::nueva();
    let r = pedir(p.app().puerto(), "GET", "/", &[], "").unwrap();
    assert_eq!(r.estado, 200);
    assert!(r.cuerpo.contains("<title>Vigía</title>"));
    assert!(r.cuerpo.contains("X-Vigia-Token"));
    assert!(
        r.cabecera
            .contains("content-security-policy: default-src 'none'")
    );
    assert!(r.cabecera.contains("text/html"));
    // La página no carga nada de internet.
    assert!(!r.cuerpo.contains("https://"));
    assert!(!r.cuerpo.contains("http://"));
}

#[test]
fn rechaza_host_origen_y_token_incorrectos() {
    let p = Prueba::nueva();
    let puerto = p.app().puerto();
    let token = p.app().token().to_string();
    let t = ("X-Vigia-Token", token.as_str());

    // Host ajeno (DNS rebinding): 403 aunque traiga el token correcto.
    let r = pedir(
        puerto,
        "GET",
        "/api/estado",
        &[("Host", "malo.example:80"), t],
        "",
    )
    .unwrap();
    assert_eq!(r.estado, 403);
    let r = pedir(puerto, "GET", "/", &[("Host", "malo.example")], "").unwrap();
    assert_eq!(r.estado, 403);
    // Origen ajeno: 403.
    let r = pedir(
        puerto,
        "GET",
        "/api/estado",
        &[("Origin", "http://malo.example"), t],
        "",
    )
    .unwrap();
    assert_eq!(r.estado, 403);
    let r = pedir(puerto, "POST", "/api/apagar", &[("Origin", "null"), t], "").unwrap();
    assert_eq!(r.estado, 403);
    // Sin token o con token incorrecto: 401.
    let r = pedir(puerto, "GET", "/api/estado", &[], "").unwrap();
    assert_eq!(r.estado, 401);
    let mut malo = token.clone();
    malo.replace_range(0..1, if token.starts_with('0') { "1" } else { "0" });
    let r = pedir(puerto, "GET", "/api/ahora", &[("X-Vigia-Token", &malo)], "").unwrap();
    assert_eq!(r.estado, 401);
    // Origen propio y localhost: aceptados.
    let origen = format!("http://127.0.0.1:{puerto}");
    let r = pedir(puerto, "GET", "/api/estado", &[("Origin", &origen), t], "").unwrap();
    assert_eq!(r.estado, 200);
    let host = format!("localhost:{puerto}");
    let r = pedir(puerto, "GET", "/api/estado", &[("Host", &host), t], "").unwrap();
    assert_eq!(r.estado, 200);
    // El intento de apagar con Origin ajeno no apagó nada.
    assert_eq!(p.get("/api/estado").estado, 200);
}

#[test]
fn la_api_entrega_procesos_e_historial() {
    let p = Prueba::nueva();
    let v = p.esperar_muestras(2);
    let procesos = v["muestra"]["procesos"].as_array().unwrap();
    let yo = std::process::id();
    let propio = procesos
        .iter()
        .find(|x| x["pid"] == yo)
        .expect("el proceso de la prueba aparece en /api/ahora");
    assert!(propio["memoria"].as_u64().unwrap() > 0);
    assert!(v["muestra"]["sistema"]["mem_total"].as_u64().unwrap() > 0);

    let inicio = propio["inicio_ms"].as_i64().unwrap();
    let d = p.json(&format!("/api/proceso?pid={yo}&inicio={inicio}"));
    assert!(!d["serie"].as_array().unwrap().is_empty());

    let estado = p.json("/api/estado");
    assert_eq!(estado["pid"], yo);

    // Las consultas del historial responden (vacías: la base es nueva).
    assert!(p.json("/api/historial/linea").as_array().is_some());
    assert!(p.json("/api/historial/momento").is_null());
    let apps = p.json("/api/apps?orden=memoria");
    assert!(apps["apps"].as_array().is_some());

    // Parámetro inválido → 400; ruta inexistente → 404.
    assert_eq!(p.get("/api/apps?desde=abc").estado, 400);
    assert_eq!(p.get("/api/no-existe").estado, 404);
}

#[test]
fn apagar_detiene_la_app() {
    let p = Prueba::nueva();
    let token = p.app().token().to_string();
    let r = pedir(
        p.app().puerto(),
        "POST",
        "/api/apagar",
        &[("X-Vigia-Token", &token)],
        "",
    )
    .unwrap();
    assert_eq!(r.estado, 200);
    let limite = Instant::now() + Duration::from_secs(5);
    while !p.app().estado().detenido() {
        assert!(Instant::now() < limite);
        std::thread::sleep(Duration::from_millis(50));
    }
}

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

#[test]
fn analisis_de_disco_por_la_api() {
    let p = Prueba::nueva();
    let puerto = p.app().puerto();
    let token = p.app().token().to_string();
    let t = [("X-Vigia-Token", token.as_str())];
    let raiz = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("arbol_api");
    std::fs::create_dir_all(raiz.join("sub")).unwrap();
    std::fs::write(raiz.join("sub").join("a.bin"), vec![0u8; 3000]).unwrap();
    std::fs::write(raiz.join("b.txt"), vec![0u8; 500]).unwrap();
    let cuerpo = serde_json::json!({ "ruta": raiz.to_string_lossy() }).to_string();

    // Ruta inválida → 400.
    let malo = serde_json::json!({ "ruta": r"no\es\absoluta" }).to_string();
    let r = pedir(puerto, "POST", "/api/disco/analizar", &t, &malo).unwrap();
    assert_eq!(r.estado, 400);

    let r = pedir(puerto, "POST", "/api/disco/analizar", &t, &cuerpo).unwrap();
    assert_eq!(r.estado, 200, "{}", r.cuerpo);
    let limite = Instant::now() + Duration::from_secs(20);
    loop {
        let prog = p.json("/api/disco/progreso");
        if prog["en_curso"] == false && !prog["ultimo_id"].is_null() {
            break;
        }
        assert!(Instant::now() < limite, "el análisis no terminó");
        std::thread::sleep(Duration::from_millis(50));
    }
    let d = p.json("/api/disco/analisis");
    let a = &d["analisis"];
    assert_eq!(a["total"], 3500);
    assert_eq!(a["archivos"], 2);
    assert_eq!(a["completo"], true);
    assert_eq!(p.json("/api/disco/lista").as_array().unwrap().len(), 1);

    // Abrir en el Explorador solo acepta rutas del análisis: una ruta ajena
    // se rechaza sin abrir nada.
    let id = a["id"].as_i64().unwrap();
    let ajena = serde_json::json!({ "id": id, "ruta": r"C:\Windows\System32" }).to_string();
    let r = pedir(puerto, "POST", "/api/disco/abrir", &t, &ajena).unwrap();
    assert_eq!(r.estado, 400);
    let sin_id = serde_json::json!({ "ruta": r"C:\x" }).to_string();
    let r = pedir(puerto, "POST", "/api/disco/abrir", &t, &sin_id).unwrap();
    assert_eq!(r.estado, 400);
}

#[test]
fn inicio_conexiones_y_avisos_por_la_api() {
    let p = Prueba::nueva();
    p.esperar_muestras(1);
    let puerto = p.app().puerto();
    let token = p.app().token().to_string();
    let t = [("X-Vigia-Token", token.as_str())];

    assert!(p.json("/api/inicio").as_array().is_some());

    // Un puerto propio en escucha aparece con el nombre del proceso.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mio = l.local_addr().unwrap().port();
    let c = p.json("/api/conexiones");
    let escucha = c["escucha"].as_array().unwrap();
    let fila = escucha
        .iter()
        .find(|x| x["puerto"] == mio && x["pid"] == std::process::id())
        .expect("el puerto de la prueba aparece en escucha");
    assert_eq!(fila["expuesto"], false);
    assert!(
        fila["proceso"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .ends_with(".exe")
    );

    // Por omisión solo avisan los puertos accesibles desde la red; esta
    // prueba abre puertos en 127.0.0.1, así que activa también los locales.
    let mut r = p.json("/api/reglas");
    r["puerto_solo_red"] = serde_json::json!(false);
    let g = pedir(puerto, "POST", "/api/reglas", &t, &r.to_string()).unwrap();
    assert_eq!(g.estado, 200);
    // Revisión forzada: la primera solo guarda la referencia.
    let r = pedir(puerto, "POST", "/api/revisar", &t, "").unwrap();
    assert_eq!(r.estado, 200, "{}", r.cuerpo);
    // Un puerto nuevo después de la referencia genera un aviso.
    let l2 = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let nuevo = l2.local_addr().unwrap().port();
    let r = pedir(puerto, "POST", "/api/revisar", &t, "").unwrap();
    assert_eq!(r.estado, 200);
    let a = p.json("/api/avisos");
    let lista = a["avisos"].as_array().unwrap();
    assert!(
        lista.iter().any(|x| x["tipo"] == "puerto_nuevo_local"
            && x["titulo"].as_str().unwrap().contains(&nuevo.to_string())),
        "{a}"
    );
    assert!(a["sin_ver"].as_i64().unwrap() >= 1);
    let r = pedir(puerto, "POST", "/api/avisos/vistos", &t, "").unwrap();
    assert_eq!(r.estado, 200);
    assert_eq!(p.json("/api/avisos")["sin_ver"], 0);
    drop((l, l2));
}

#[test]
fn parametros_extremos_no_rompen_el_servidor() {
    let p = Prueba::nueva();
    for ruta in [
        "/api/historial/linea?hasta=-9223372036854775808",
        "/api/historial/linea?desde=9223372036854775807&hasta=-9223372036854775808",
        "/api/apps?hasta=-9223372036854775808",
        "/api/historial/momento?ts=9223372036854775807",
    ] {
        assert_eq!(p.get(ruta).estado, 200, "{ruta}");
    }
    // El servidor sigue respondiendo.
    assert_eq!(p.get("/api/estado").estado, 200);
}

#[test]
fn un_cliente_sin_token_que_no_envia_el_cuerpo_no_bloquea() {
    use std::io::Write;
    let p = Prueba::nueva();
    let puerto = p.app().puerto();
    // Anuncia 60 KB de cuerpo y no envía nada.
    let mut lento = std::net::TcpStream::connect(("127.0.0.1", puerto)).unwrap();
    write!(
        lento,
        "POST /api/apagar HTTP/1.1\r\nHost: 127.0.0.1:{puerto}\r\nContent-Length: 60000\r\n\r\n"
    )
    .unwrap();
    // Otras peticiones siguen funcionando.
    let inicio = Instant::now();
    assert_eq!(p.get("/api/estado").estado, 200);
    assert!(inicio.elapsed() < Duration::from_secs(5));
    drop(lento);
}

#[test]
fn reglas_por_la_api_y_aviso_real_de_disco() {
    let p = Prueba::nueva();
    let puerto = p.app().puerto();
    let token = p.app().token().to_string();
    let t = [("X-Vigia-Token", token.as_str())];

    let mut r = p.json("/api/reglas");
    assert_eq!(r["memoria_pct"], 90.0);

    // Cuerpo inválido → 400.
    let malo = pedir(puerto, "POST", "/api/reglas", &t, "no es json").unwrap();
    assert_eq!(malo.estado, 400);

    // Valores fuera de rango se corrigen; desactivar avisos de puertos.
    r["memoria_pct"] = serde_json::json!(500);
    r["puerto_activa"] = serde_json::json!(false);
    // Umbral de disco al 99 %: cualquier unidad con menos de 99 % libre avisa.
    r["disco_pct"] = serde_json::json!(99);
    let g = pedir(puerto, "POST", "/api/reglas", &t, &r.to_string()).unwrap();
    assert_eq!(g.estado, 200, "{}", g.cuerpo);
    let guardadas = p.json("/api/reglas");
    assert_eq!(guardadas["memoria_pct"], 100.0);
    assert_eq!(guardadas["puerto_activa"], false);

    // Con datos reales, el motor avisa de poco espacio en alguna unidad.
    let limite = Instant::now() + Duration::from_secs(10);
    loop {
        let a = p.json("/api/avisos");
        if a["avisos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["tipo"] == "disco_poco")
        {
            break;
        }
        assert!(Instant::now() < limite, "no llegó el aviso de disco");
        std::thread::sleep(Duration::from_millis(200));
    }

    // Con la regla de puertos desactivada, un puerto nuevo no avisa.
    pedir(puerto, "POST", "/api/revisar", &t, "").unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    pedir(puerto, "POST", "/api/revisar", &t, "").unwrap();
    let a = p.json("/api/avisos");
    assert!(
        !a["avisos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["tipo"].as_str().unwrap().starts_with("puerto_nuevo"))
    );
    drop(l);
}

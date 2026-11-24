//! Utilidades del sistema: números aleatorios seguros, abrir el navegador o
//! el Explorador, y el aviso de cierre de la ventana de consola.

use super::{Resultado, a_utf16};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{BOOL, PCWSTR, w};

/// Llena `bytes` con números aleatorios criptográficamente seguros.
pub fn aleatorio(bytes: &mut [u8]) -> Resultado<()> {
    // BCryptGenRandom (bcrypt.dll) con BCRYPT_USE_SYSTEM_PREFERRED_RNG: usa
    // el generador aleatorio del sistema sin tener que abrir un "proveedor
    // de algoritmo" antes. Es el reemplazo moderno de RtlGenRandom y es
    // apto para secretos como el token de la interfaz.
    unsafe { BCryptGenRandom(None, bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.ok()
}

/// Abre un archivo o una URL con el programa predeterminado (por ejemplo,
/// el navegador para `http://...`).
pub fn abrir(destino: &str) -> Resultado<()> {
    ejecutar(destino, None)
}

/// Abre el Explorador de Windows con `ruta` seleccionada.
pub fn mostrar_en_explorador(ruta: &str) -> Resultado<()> {
    // `/select,"ruta"` le pide al Explorador abrir la carpeta que contiene
    // `ruta` y dejarla marcada. No borra ni mueve nada.
    let parametros = format!("/select,\"{ruta}\"");
    ejecutar("explorer.exe", Some(&parametros))
}

fn ejecutar(archivo: &str, parametros: Option<&str>) -> Resultado<()> {
    let archivo_w = a_utf16(archivo);
    let parametros_w = parametros.map(a_utf16);
    // ShellExecuteW (shell32): "haz lo que haría un doble clic". Con el
    // verbo "open" y una URL abre el navegador predeterminado; con un .exe y
    // parámetros, lo ejecuta. Devuelve un valor > 32 si tuvo éxito (es una
    // API antigua de 16 bits que mezcla errores con manijas).
    let r = unsafe {
        ShellExecuteW(
            Some(HWND::default()),
            w!("open"),
            PCWSTR(archivo_w.as_ptr()),
            match &parametros_w {
                Some(p) => PCWSTR(p.as_ptr()),
                None => PCWSTR::null(),
            },
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if r.0 as usize > 32 {
        Ok(())
    } else {
        Err(windows::core::Error::from_thread())
    }
}

/// Se pone en `true` cuando el usuario cierra la ventana de consola,
/// presiona Ctrl+C o cierra la sesión de Windows.
static CIERRE_PEDIDO: AtomicBool = AtomicBool::new(false);
/// El programa lo pone en `true` cuando terminó de guardar todo.
static CIERRE_LISTO: AtomicBool = AtomicBool::new(false);

/// Función que Windows llama (en otro hilo) ante Ctrl+C o cierre de consola.
unsafe extern "system" fn al_cerrar_consola(_tipo: u32) -> BOOL {
    CIERRE_PEDIDO.store(true, Ordering::SeqCst);
    // Si devolvemos enseguida, Windows termina el proceso al instante en el
    // caso de "cerrar ventana". Esperamos hasta 4 s a que se guarde el
    // último minuto del historial.
    let limite = Instant::now() + Duration::from_secs(4);
    while !CIERRE_LISTO.load(Ordering::SeqCst) && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(50));
    }
    true.into() // TRUE = "lo manejamos nosotros".
}

/// Registra el aviso de cierre de consola.
pub fn escuchar_cierre_consola() -> Resultado<()> {
    // SetConsoleCtrlHandler (kernel32): agrega una función a la lista de las
    // que Windows llama cuando llega Ctrl+C, Ctrl+Inter, se cierra la
    // ventana o se cierra la sesión.
    unsafe { SetConsoleCtrlHandler(Some(al_cerrar_consola), true) }
}

pub fn cierre_pedido() -> bool {
    CIERRE_PEDIDO.load(Ordering::SeqCst)
}

pub fn marcar_cierre_listo() {
    CIERRE_LISTO.store(true, Ordering::SeqCst);
}

/// Evita que Windows muestre cuadros de diálogo como «No hay disco en la
/// unidad» cuando Vigía consulta un lector de tarjetas vacío. Afecta solo
/// a este proceso.
pub fn sin_dialogos_de_error() {
    use windows::Win32::System::Diagnostics::Debug::{
        SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetErrorMode,
    };
    // SetErrorMode (kernel32): con SEM_FAILCRITICALERRORS los errores
    // "críticos" (como un disco ausente) se devuelven como error a quien
    // llamó, en lugar de mostrar un cuadro al usuario.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX);
    }
}

/// Bloqueo de "una sola instancia": mientras exista, otro `vigia.exe` con
/// la misma carpeta de datos no puede crear el suyo.
pub struct InstanciaUnica(windows::Win32::Foundation::HANDLE);

impl Drop for InstanciaUnica {
    fn drop(&mut self) {
        // CloseHandle (kernel32): al cerrar la última manija, Windows borra
        // el mutex y otra instancia ya puede arrancar.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Intenta tomar el bloqueo `nombre`. Devuelve `None` si otra instancia ya
/// lo tiene.
pub fn instancia_unica(nombre: &str) -> Resultado<Option<InstanciaUnica>> {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    let nombre_w = a_utf16(nombre);
    // CreateMutexW (kernel32): crea un mutex con nombre, o abre el existente.
    // Es una operación atómica del sistema: si dos procesos lo intentan a la
    // vez, solo uno lo crea y el otro recibe ERROR_ALREADY_EXISTS. El prefijo
    // "Local\" lo limita a la sesión del usuario actual.
    let h = unsafe { CreateMutexW(None, false, PCWSTR(nombre_w.as_ptr()))? };
    // GetLastError (kernel32): CreateMutexW tiene éxito también cuando el
    // mutex ya existía; la diferencia se ve en el último error.
    let ya_existia = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let bloqueo = InstanciaUnica(h);
    Ok((!ya_existia).then_some(bloqueo))
}

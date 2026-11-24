//! Notificación propia: una ventanita en la esquina inferior derecha que
//! se cierra sola.
//!
//! ## Por qué no la notificación "nativa" de Windows
//! - Los globos de `Shell_NotifyIconW` hacen que el Explorador registre el
//!   ícono en `HKCU\Control Panel\NotifyIconSettings`.
//! - Las notificaciones *toast* exigen un AppUserModelID registrado en el
//!   registro o un acceso directo en el menú Inicio.
//!
//! Vigía no debe escribir en el registro, ni directa ni indirectamente, así
//! que dibuja su propia ventana con Win32 y GDI. De paso, es un buen ejemplo
//! de cómo funciona una ventana de Windows por dentro: clase, procedimiento
//! de ventana, mensajes y pintado.

use super::{Resultado, a_utf16};
use std::sync::{Mutex, Once};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE,
    DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint, FillRect, HGDIOBJ, PAINTSTRUCT, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetMessageW, GetWindowLongPtrW, IDC_HAND, LoadCursorW, MSG, PostQuitMessage, RegisterClassExW,
    SPI_GETWORKAREA, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetTimer,
    SetWindowLongPtrW, ShowWindow, SystemParametersInfoW, TranslateMessage, WM_DESTROY,
    WM_LBUTTONUP, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_BORDER,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const ANCHO: i32 = 380;
const ALTO: i32 = 100;
const MARGEN: i32 = 12;
/// Cuántas notificaciones pueden verse a la vez (apiladas).
const LUGARES: usize = 3;

/// Lugares ocupados en la pila de notificaciones.
static OCUPADOS: Mutex<[bool; LUGARES]> = Mutex::new([false; LUGARES]);
static REGISTRAR_CLASE: Once = Once::new();
const CLASE: PCWSTR = w!("VigiaNotificacion");

/// Lo que la ventana necesita para pintarse. Viaja como puntero dentro de
/// la ventana (GWLP_USERDATA) y se libera al destruirla.
struct Contenido {
    titulo: Vec<u16>,
    texto: Vec<u16>,
}

/// Colores GDI: COLORREF guarda los canales al revés (0x00BBGGRR).
const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((b as u32) << 16 | (g as u32) << 8 | r as u32)
}

/// Muestra una notificación y **bloquea** hasta que se cierra (al pasar
/// `duracion_ms` o con un clic). Llamarla desde un hilo propio.
/// Devuelve `Ok(false)` sin mostrar nada si ya hay `LUGARES` en pantalla.
pub fn mostrar(titulo: &str, texto: &str, duracion_ms: u32) -> Resultado<bool> {
    let lugar = {
        let mut o = OCUPADOS.lock().unwrap_or_else(|e| e.into_inner());
        match o.iter().position(|x| !x) {
            Some(i) => {
                o[i] = true;
                i
            }
            None => return Ok(false),
        }
    };
    let r = mostrar_en(lugar, titulo, texto, duracion_ms);
    OCUPADOS.lock().unwrap_or_else(|e| e.into_inner())[lugar] = false;
    r.map(|()| true)
}

fn mostrar_en(lugar: usize, titulo: &str, texto: &str, duracion_ms: u32) -> Resultado<()> {
    // GetModuleHandleW(None) (kernel32): manija de nuestro propio .exe; las
    // clases de ventana se registran "a nombre" de un módulo.
    let instancia = unsafe { GetModuleHandleW(None)? };
    REGISTRAR_CLASE.call_once(|| {
        let clase = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedimiento),
            hInstance: instancia.into(),
            // LoadCursorW (user32): cursor de "mano" para indicar que un
            // clic la cierra.
            hCursor: unsafe { LoadCursorW(None, IDC_HAND) }.unwrap_or_default(),
            lpszClassName: CLASE,
            ..Default::default()
        };
        // RegisterClassExW (user32): registra el "molde" de ventana con su
        // procedimiento (la función que recibe todos sus mensajes).
        unsafe { RegisterClassExW(&clase) };
    });

    // SystemParametersInfoW(SPI_GETWORKAREA) (user32): rectángulo de la
    // pantalla principal sin la barra de tareas, para ubicarnos encima.
    let mut area = RECT::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut area).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?
    };
    let x = area.right - ANCHO - MARGEN;
    let y = area.bottom - (ALTO + MARGEN) * (lugar as i32 + 1);

    let contenido = Box::new(Contenido {
        titulo: titulo.encode_utf16().collect(),
        texto: texto.encode_utf16().collect(),
    });
    let titulo_ventana = a_utf16(titulo);
    // CreateWindowExW (user32): crea la ventana.
    // - WS_POPUP | WS_BORDER: sin barra de título, con un borde fino.
    // - WS_EX_TOPMOST: queda encima de las demás.
    // - WS_EX_TOOLWINDOW: no aparece en la barra de tareas ni en Alt+Tab.
    // - WS_EX_NOACTIVATE: no roba el foco a lo que estés escribiendo.
    // El último parámetro llega a WM_NCCREATE: así le pasamos el texto.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASE,
            PCWSTR(titulo_ventana.as_ptr()),
            WS_POPUP | WS_BORDER,
            x,
            y,
            ANCHO,
            ALTO,
            None,
            None,
            Some(instancia.into()),
            Some(Box::into_raw(contenido).cast()),
        )?
    };
    unsafe {
        // ShowWindow con SW_SHOWNOACTIVATE: mostrar sin activar.
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // SetTimer (user32): en `duracion_ms` la ventana recibe WM_TIMER.
        SetTimer(Some(hwnd), 1, duracion_ms, None);
    }
    // Bucle de mensajes: GetMessageW espera el siguiente mensaje de este
    // hilo y DispatchMessageW lo entrega a `procedimiento`. Termina cuando
    // llega WM_QUIT (que enviamos con PostQuitMessage al destruirla).
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

/// Procedimiento de ventana: Windows lo llama con cada mensaje.
unsafe extern "system" fn procedimiento(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                // Guardamos el puntero al contenido dentro de la ventana.
                let cs = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_PAINT => {
                let c = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Contenido;
                if !c.is_null() {
                    pintar(hwnd, &mut *c);
                }
                LRESULT(0)
            }
            WM_TIMER | WM_LBUTTONUP => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                // Último mensaje de la ventana: devolvemos el contenido a un
                // Box para que Rust libere su memoria.
                let c = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Contenido;
                if !c.is_null() {
                    drop(Box::from_raw(c));
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Dibuja fondo, franja de color, título y texto con GDI.
unsafe fn pintar(hwnd: HWND, c: &mut Contenido) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        // BeginPaint / EndPaint (user32): dan el "contexto de dispositivo"
        // (HDC) donde se dibuja y marcan la ventana como ya pintada.
        let hdc = BeginPaint(hwnd, &mut ps);
        let todo = RECT {
            left: 0,
            top: 0,
            right: ANCHO,
            bottom: ALTO,
        };
        // CreateSolidBrush + FillRect (gdi32/user32): pintar el fondo.
        let fondo = CreateSolidBrush(rgb(24, 28, 36));
        FillRect(hdc, &todo, fondo);
        let franja = CreateSolidBrush(rgb(91, 156, 255));
        FillRect(hdc, &RECT { right: 5, ..todo }, franja);
        SetBkMode(hdc, TRANSPARENT);

        // CreateFontW (gdi32): fuente Segoe UI. Altura negativa = tamaño
        // del carácter en píxeles.
        let negrita = CreateFontW(
            -16,
            0,
            0,
            0,
            600,
            0,
            0,
            0,
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            0,
            w!("Segoe UI"),
        );
        let normal = CreateFontW(
            -14,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            0,
            w!("Segoe UI"),
        );
        // SelectObject: elige la fuente con la que DrawTextW escribe.
        let anterior = SelectObject(hdc, HGDIOBJ(negrita.0));
        SetTextColor(hdc, rgb(255, 255, 255));
        let mut r = RECT {
            left: 18,
            top: 12,
            right: ANCHO - 14,
            bottom: 36,
        };
        // DrawTextW (user32): escribe texto dentro de un rectángulo; con
        // DT_END_ELLIPSIS agrega "…" si no cabe.
        DrawTextW(
            hdc,
            &mut c.titulo,
            &mut r,
            DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, HGDIOBJ(normal.0));
        SetTextColor(hdc, rgb(200, 206, 216));
        let mut r = RECT {
            left: 18,
            top: 38,
            right: ANCHO - 14,
            bottom: ALTO - 10,
        };
        DrawTextW(
            hdc,
            &mut c.texto,
            &mut r,
            DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        // Devolver la fuente original y liberar los objetos GDI creados
        // (si no, el proceso va acumulando objetos GDI).
        SelectObject(hdc, anterior);
        let _ = DeleteObject(HGDIOBJ(negrita.0));
        let _ = DeleteObject(HGDIOBJ(normal.0));
        let _ = DeleteObject(HGDIOBJ(fondo.0));
        let _ = DeleteObject(HGDIOBJ(franja.0));
        let _ = EndPaint(hwnd, &ps);
    }
}

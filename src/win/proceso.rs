//! Lista de procesos con sus contadores de CPU, memoria y E/S.
//!
//! ## Por qué `NtQuerySystemInformation`
//!
//! La forma "de libro" sería: listar PIDs y luego abrir cada proceso con
//! `OpenProcess` para preguntarle sus tiempos y su memoria. Pero sin
//! permisos de administrador Windows **niega** abrir muchos procesos del
//! sistema (en una prueba, 146 de 370), y además son cuatro o cinco
//! llamadas por proceso en cada muestra.
//!
//! `NtQuerySystemInformation(SystemProcessInformation)` es lo que usa el
//! propio Administrador de tareas: **una sola llamada** devuelve una lista
//! encadenada con nombre, PID, tiempos de CPU, memoria y bytes de E/S de
//! *todos* los procesos, sin abrir ninguno y sin necesitar administrador.
//! Está declarada en `winternl.h` (documentación de Microsoft) con varios
//! campos marcados como "reservados"; aquí se leen siguiendo la estructura
//! pública que documentan herramientas como Process Hacker/System Informer.
//!
//! La ruta completa del ejecutable no viene en esa lista; para ella sí se
//! abre el proceso (ver [`ruta`]), y el colector la guarda en caché.

use super::{Manija, Resultado};
use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessInformation};
use windows::Win32::Foundation::{NTSTATUS, STATUS_INFO_LENGTH_MISMATCH};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::System::WindowsProgramming::SYSTEM_PROCESS_INFORMATION;
use windows::core::PWSTR;

/// Un proceso tal como lo entrega Windows: contadores **acumulados**.
#[derive(Debug, Clone, Default)]
pub struct EntradaProceso {
    pub pid: u32,
    /// PID del proceso que lo creó (puede que ya no exista).
    pub ppid: u32,
    /// Nombre del ejecutable, por ejemplo `chrome.exe`.
    pub nombre: String,
    pub hilos: u32,
    pub manijas: u32,
    /// Hora de creación, ms desde 1970 (UTC). `None` para el PID 0.
    pub inicio_ms: Option<i64>,
    /// Tiempo de CPU acumulado (núcleo + usuario), en unidades de 100 ns.
    pub cpu_100ns: u64,
    /// *Working set*: memoria física en uso, incluida la compartida.
    pub memoria_fisica: u64,
    /// *Private working set*: memoria física que solo usa este proceso.
    /// Es la columna «Memoria» del Administrador de tareas.
    pub memoria_privada_fisica: u64,
    /// Memoria privada comprometida (puede estar en RAM o en el archivo de
    /// paginación).
    pub memoria_comprometida: u64,
    /// Bytes leídos y escritos acumulados (disco, red y dispositivos).
    pub io_lectura: u64,
    pub io_escritura: u64,
}

/// Lee la lista de procesos. Guarda un búfer entre llamadas para no pedir
/// memoria nueva en cada muestra.
#[derive(Default)]
pub struct LectorProcesos {
    /// `u64` en lugar de `u8` para que el búfer quede alineado a 8 bytes,
    /// como exigen las estructuras que Windows escribe dentro.
    bufer: Vec<u64>,
}

impl LectorProcesos {
    pub fn leer(&mut self) -> Resultado<Vec<EntradaProceso>> {
        if self.bufer.is_empty() {
            self.bufer = vec![0u64; 512 * 1024 / 8]; // 512 KB para empezar
        }
        // La lista cambia de tamaño a cada instante. Si el búfer no alcanza,
        // Windows responde STATUS_INFO_LENGTH_MISMATCH y dice cuánto
        // necesita; agrandamos con margen y reintentamos.
        let mut intentos = 0;
        loop {
            let mut necesario = 0u32;
            let bytes = (self.bufer.len() * 8) as u32;
            // NtQuerySystemInformation (ntdll) con SystemProcessInformation:
            // escribe en el búfer una SYSTEM_PROCESS_INFORMATION por proceso,
            // encadenadas por `NextEntryOffset`.
            let estado: NTSTATUS = unsafe {
                NtQuerySystemInformation(
                    SystemProcessInformation,
                    self.bufer.as_mut_ptr().cast(),
                    bytes,
                    &mut necesario,
                )
            };
            if estado == STATUS_INFO_LENGTH_MISMATCH && intentos < 5 {
                intentos += 1;
                let nuevo = (necesario as usize).max(bytes as usize * 2) + 64 * 1024;
                self.bufer = vec![0u64; nuevo / 8 + 1];
                continue;
            }
            estado.ok()?;
            // Si todo salió bien, `necesario` es la cantidad de bytes que
            // Windows escribió: recorremos solo esos, no el búfer entero.
            let escritos = if necesario == 0 { bytes } else { necesario };
            return Ok(self.recorrer((escritos as usize).min(bytes as usize)));
        }
    }

    /// Recorre la lista encadenada que Windows dejó en los primeros `largo`
    /// bytes del búfer.
    fn recorrer(&self, largo: usize) -> Vec<EntradaProceso> {
        let base = self.bufer.as_ptr().cast::<u8>();
        let rango = base as usize..base as usize + largo;
        let tam_entrada = std::mem::size_of::<SYSTEM_PROCESS_INFORMATION>();
        let mut lista = Vec::with_capacity(400);
        let mut desplazamiento = 0usize;
        while desplazamiento + tam_entrada <= largo {
            // SAFETY: comprobamos arriba que la estructura completa cabe en
            // el búfer, y Windows alinea cada entrada a 8 bytes.
            let info = unsafe {
                &*base
                    .add(desplazamiento)
                    .cast::<SYSTEM_PROCESS_INFORMATION>()
            };
            lista.push(convertir(info, &rango));
            if info.NextEntryOffset == 0 {
                break;
            }
            desplazamiento += info.NextEntryOffset as usize;
        }
        lista
    }
}

/// Lee los campos de una entrada, incluidos los "reservados".
/// `rango` son las direcciones válidas del búfer: el nombre del proceso
/// debe caer dentro de él.
fn convertir(info: &SYSTEM_PROCESS_INFORMATION, rango: &std::ops::Range<usize>) -> EntradaProceso {
    // `Reserved1` (48 bytes) contiene, en este orden:
    //   0..8   WorkingSetPrivateSize   (memoria física privada)
    //   8..12  HardFaultCount
    //  12..16  NumberOfThreadsHighWatermark
    //  16..24  CycleTime
    //  24..32  CreateTime  (FILETIME)
    //  32..40  UserTime    (100 ns)
    //  40..48  KernelTime  (100 ns)
    let r = &info.Reserved1;
    let leer_u64 = |i: usize| {
        let mut b = [0u8; 8];
        b.copy_from_slice(&r[i..i + 8]);
        u64::from_le_bytes(b)
    };
    let privada_fisica = leer_u64(0);
    let creado = leer_u64(24);
    let usuario = leer_u64(32);
    let nucleo = leer_u64(40);

    // `Reserved7` son seis contadores de E/S: operaciones de lectura,
    // escritura y otras, y luego bytes de lectura, escritura y otros.
    let io_lectura = info.Reserved7[3].max(0) as u64;
    let io_escritura = info.Reserved7[4].max(0) as u64;

    // El nombre es un UNICODE_STRING: puntero + largo en **bytes**, sin cero
    // final. El puntero apunta dentro de nuestro mismo búfer.
    let inicio_nombre = info.ImageName.Buffer.0 as usize;
    let fin_nombre = inicio_nombre + usize::from(info.ImageName.Length);
    let nombre_valido = rango.contains(&inicio_nombre) && fin_nombre <= rango.end;
    let nombre = if !nombre_valido || info.ImageName.Length == 0 {
        if info.UniqueProcessId.0.is_null() {
            "Proceso inactivo del sistema".to_string()
        } else {
            String::new()
        }
    } else {
        // SAFETY: comprobamos arriba que `Length` bytes desde `Buffer`
        // están dentro de nuestro búfer.
        let s = unsafe {
            std::slice::from_raw_parts(
                info.ImageName.Buffer.0,
                usize::from(info.ImageName.Length) / 2,
            )
        };
        String::from_utf16_lossy(s)
    };

    EntradaProceso {
        // Windows guarda los PID como HANDLE (un puntero); son números chicos.
        pid: info.UniqueProcessId.0 as usize as u32,
        // `Reserved2` es InheritedFromUniqueProcessId: el PID del padre.
        ppid: info.Reserved2 as usize as u32,
        nombre,
        hilos: info.NumberOfThreads,
        manijas: info.HandleCount,
        inicio_ms: if creado == 0 {
            None
        } else {
            super::filetime_a_unix_ms(windows::Win32::Foundation::FILETIME {
                dwLowDateTime: creado as u32,
                dwHighDateTime: (creado >> 32) as u32,
            })
        },
        cpu_100ns: usuario + nucleo,
        memoria_fisica: info.WorkingSetSize as u64,
        memoria_privada_fisica: privada_fisica,
        // PrivatePageCount, pese al nombre, viene en bytes.
        memoria_comprometida: info.PrivatePageCount as u64,
        io_lectura,
        io_escritura,
    }
}

/// Ruta completa del ejecutable de un proceso, si Windows deja leerla.
///
/// Primero intenta abriendo el proceso (lo documentado y habitual). Si
/// Windows lo niega (servicios del sistema sin administrador), pregunta la
/// ruta al núcleo sin abrir el proceso (ver [`ruta_sin_abrir`]).
pub fn ruta(pid: u32) -> Option<String> {
    if pid == 0 || pid == 4 {
        return None; // Inactivo y System no tienen archivo ejecutable propio.
    }
    ruta_abriendo(pid).or_else(|| ruta_sin_abrir(pid))
}

/// Clase 88 de NtQuerySystemInformation: `SystemProcessIdInformation`.
/// No está en el crate `windows`, así que se declara a mano.
const SYSTEM_PROCESS_ID_INFORMATION_CLASS: i32 = 88;

/// Estructura que pide la clase 88: le damos el PID y un búfer vacío, y
/// Windows llena `ImageName` con la ruta en formato de dispositivo.
#[repr(C)]
struct SystemProcessIdInformation {
    process_id: windows::Win32::Foundation::HANDLE,
    image_name: windows::Win32::Foundation::UNICODE_STRING,
}

/// Ruta del ejecutable **sin abrir el proceso**.
///
/// `NtQuerySystemInformation(SystemProcessIdInformation)` (ntdll) devuelve
/// la ruta de cualquier proceso, incluso de servicios que `OpenProcess` nos
/// niega, pero en formato del núcleo: `\Device\HarddiskVolume3\Windows\...`.
/// La pasamos a `C:\Windows\...` con [`dispositivo_a_dos`].
fn ruta_sin_abrir(pid: u32) -> Option<String> {
    let mut bufer = vec![0u16; 1024];
    let mut info = SystemProcessIdInformation {
        process_id: windows::Win32::Foundation::HANDLE(pid as usize as *mut _),
        image_name: windows::Win32::Foundation::UNICODE_STRING {
            Length: 0,
            MaximumLength: (bufer.len() * 2) as u16,
            Buffer: PWSTR(bufer.as_mut_ptr()),
        },
    };
    let estado = unsafe {
        NtQuerySystemInformation(
            windows::Wdk::System::SystemInformation::SYSTEM_INFORMATION_CLASS(
                SYSTEM_PROCESS_ID_INFORMATION_CLASS,
            ),
            (&raw mut info).cast(),
            std::mem::size_of::<SystemProcessIdInformation>() as u32,
            std::ptr::null_mut(),
        )
    };
    if estado.is_err() {
        return None;
    }
    let largo = (usize::from(info.image_name.Length) / 2).min(bufer.len());
    let nt = String::from_utf16_lossy(&bufer[..largo]);
    if nt.is_empty() {
        return None;
    }
    Some(dispositivo_a_dos(&nt).unwrap_or(nt))
}

/// Convierte `\Device\HarddiskVolume3\x` en `C:\x` probando cada letra.
fn dispositivo_a_dos(ruta_nt: &str) -> Option<String> {
    tabla_de_unidades()
        .into_iter()
        .find_map(|(unidad, dispositivo)| {
            ruta_nt
                .strip_prefix(&dispositivo)
                .filter(|resto| resto.starts_with('\\'))
                .map(|resto| format!("{unidad}{resto}"))
        })
}

/// Pares (letra, dispositivo), p. ej. ("C:", "\Device\HarddiskVolume3").
/// Se recalcula como mucho cada 60 s: al arrancar hay unos 150 procesos que
/// necesitan esta conversión y no tiene sentido preguntar 26 letras para
/// cada uno.
fn tabla_de_unidades() -> Vec<(String, String)> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    /// (cuándo se armó, pares letra → dispositivo)
    type Tabla = Option<(Instant, Vec<(String, String)>)>;
    static CACHE: Mutex<Tabla> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cuando, tabla)) = cache.as_ref()
        && cuando.elapsed() < Duration::from_secs(60)
    {
        return tabla.clone();
    }
    let mut tabla = Vec::new();
    for letra in b'A'..=b'Z' {
        let unidad = format!("{}:", letra as char);
        let unidad_w = super::a_utf16(&unidad);
        let mut destino = [0u16; 512];
        // QueryDosDeviceW (kernel32): dice a qué dispositivo del núcleo
        // apunta una letra de unidad, p. ej. "C:" → "\Device\HarddiskVolume3".
        let n = unsafe {
            windows::Win32::Storage::FileSystem::QueryDosDeviceW(
                windows::core::PCWSTR(unidad_w.as_ptr()),
                Some(&mut destino),
            )
        };
        if n != 0 {
            tabla.push((unidad, super::desde_utf16(&destino)));
        }
    }
    *cache = Some((Instant::now(), tabla.clone()));
    tabla
}

/// Ruta abriendo el proceso con el derecho mínimo.
fn ruta_abriendo(pid: u32) -> Option<String> {
    // OpenProcess (kernel32): pide una manija al proceso con ciertos
    // derechos. PROCESS_QUERY_LIMITED_INFORMATION es el derecho mínimo y el
    // que un usuario normal obtiene sobre más procesos.
    let h = Manija(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let mut bufer = [0u16; 1024];
    let mut largo = bufer.len() as u32;
    // QueryFullProcessImageNameW (kernel32): ruta completa del ejecutable
    // en formato normal (C:\...) gracias a PROCESS_NAME_WIN32. Funciona con
    // el derecho limitado, a diferencia de GetModuleFileNameExW.
    unsafe {
        QueryFullProcessImageNameW(
            h.0,
            PROCESS_NAME_WIN32,
            PWSTR(bufer.as_mut_ptr()),
            &mut largo,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&bufer[..largo as usize]))
}

/// PID del propio Vigía.
pub fn pid_propio() -> u32 {
    // GetCurrentProcessId (kernel32): no puede fallar.
    unsafe { GetCurrentProcessId() }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn el_propio_proceso_aparece() {
        let lista = LectorProcesos::default().leer().unwrap();
        let yo = lista.iter().find(|p| p.pid == pid_propio()).unwrap();
        assert!(yo.nombre.to_lowercase().ends_with(".exe"));
        assert!(yo.memoria_fisica > 0);
        assert!(yo.inicio_ms.is_some());
        assert!(yo.hilos >= 1);
        assert!(ruta(pid_propio()).is_some());
    }

    #[test]
    fn system_pid_4_aparece_sin_ruta_y_sin_error() {
        let lista = LectorProcesos::default().leer().unwrap();
        let system = lista.iter().find(|p| p.pid == 4).unwrap();
        assert_eq!(system.nombre, "System");
        assert_eq!(ruta(4), None);
    }
}

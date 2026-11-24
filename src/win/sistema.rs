//! Totales del sistema: memoria, tiempos de CPU y unidades de disco.

use super::{Resultado, a_utf16, desde_utf16, filetime_a_u64};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};
use windows::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows::Win32::System::Threading::{GetActiveProcessorCount, GetSystemTimes};
use windows::core::PCWSTR;

/// Memoria física del equipo, en bytes.
#[derive(Debug, Clone, Copy)]
pub struct Memoria {
    pub total: u64,
    pub disponible: u64,
}

/// Lee la memoria física total y disponible.
pub fn memoria() -> Resultado<Memoria> {
    // MEMORYSTATUSEX debe llevar su propio tamaño en `dwLength`: así Windows
    // sabe qué versión de la estructura le pasamos.
    let mut st = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // GlobalMemoryStatusEx (kernel32): llena MEMORYSTATUSEX con la memoria
    // física (RAM) y virtual. Se eligió porque es la llamada oficial y barata
    // para el total del sistema; `ullAvailPhys` es lo que el Administrador de
    // tareas llama «Disponible».
    unsafe { GlobalMemoryStatusEx(&mut st)? };
    Ok(Memoria {
        total: st.ullTotalPhys,
        disponible: st.ullAvailPhys,
    })
}

/// Tiempos de CPU acumulados de todo el sistema desde el arranque,
/// en unidades de 100 ns y sumando todos los núcleos.
#[derive(Debug, Clone, Copy, Default)]
pub struct TiemposCpu {
    pub inactivo: u64,
    /// Ojo: en Windows el tiempo de núcleo (*kernel*) **incluye** el inactivo.
    pub nucleo: u64,
    pub usuario: u64,
}

/// Lee los tiempos de CPU acumulados del sistema.
///
/// Un porcentaje de uso se calcula comparando dos lecturas:
/// `ocupado = (Δnucleo + Δusuario − Δinactivo) / (Δnucleo + Δusuario)`.
pub fn tiempos_cpu() -> Resultado<TiemposCpu> {
    let mut inactivo = FILETIME::default();
    let mut nucleo = FILETIME::default();
    let mut usuario = FILETIME::default();
    // GetSystemTimes (kernel32): devuelve tres contadores acumulados de
    // tiempo inactivo, de núcleo y de usuario para todos los procesadores.
    // Es la forma más simple de obtener el % de CPU total sin contadores de
    // rendimiento (PDH), que serían más pesados.
    unsafe { GetSystemTimes(Some(&mut inactivo), Some(&mut nucleo), Some(&mut usuario))? };
    Ok(TiemposCpu {
        inactivo: filetime_a_u64(inactivo),
        nucleo: filetime_a_u64(nucleo),
        usuario: filetime_a_u64(usuario),
    })
}

/// Número de procesadores lógicos activos (todos los grupos).
pub fn procesadores() -> u32 {
    // GetActiveProcessorCount (kernel32) con ALL_PROCESSOR_GROUPS (0xFFFF):
    // cuenta los procesadores lógicos de todos los grupos. Lo necesitamos para
    // pasar el tiempo de CPU de un proceso a porcentaje del equipo completo.
    let n = unsafe { GetActiveProcessorCount(0xFFFF) };
    n.max(1)
}

/// Milisegundos desde que arrancó Windows.
pub fn ms_desde_arranque() -> u64 {
    // GetTickCount64 (kernel32): reloj monótono en ms desde el arranque.
    unsafe { GetTickCount64() }
}

/// Una unidad de disco con su espacio.
#[derive(Debug, Clone)]
pub struct Unidad {
    /// Raíz, por ejemplo `C:\`.
    pub raiz: String,
    pub etiqueta: String,
    /// "fijo" o "extraíble".
    pub tipo: &'static str,
    pub total: u64,
    pub libre: u64,
}

// Valores de GetDriveTypeW (winbase.h).
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

/// Lista las unidades fijas y extraíbles con su espacio total y libre.
///
/// Se omiten las unidades de red y de CD: consultarlas puede tardar
/// segundos o colgarse si el servidor no responde.
pub fn unidades() -> Resultado<Vec<Unidad>> {
    let mut bufer = [0u16; 512];
    // GetLogicalDriveStringsW (kernel32): escribe las raíces de todas las
    // unidades como "C:\␀D:\␀␀". Devuelve los caracteres escritos.
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut bufer)) } as usize;
    if n == 0 || n > bufer.len() {
        return Err(windows::core::Error::from_thread());
    }
    let mut resultado = Vec::new();
    for raiz in bufer[..n].split(|&c| c == 0).filter(|s| !s.is_empty()) {
        let raiz_texto = String::from_utf16_lossy(raiz);
        let raiz_w = a_utf16(&raiz_texto);
        let ptr = PCWSTR(raiz_w.as_ptr());
        // GetDriveTypeW (kernel32): dice si la unidad es fija, extraíble,
        // de red, CD-ROM, etc. Sirve para no consultar unidades lentas.
        let tipo = match unsafe { GetDriveTypeW(ptr) } {
            DRIVE_FIXED => "fijo",
            DRIVE_REMOVABLE => "extraíble",
            _ => continue,
        };
        let mut total = 0u64;
        let mut libre = 0u64;
        // GetDiskFreeSpaceExW (kernel32): bytes libres para el usuario,
        // total y libres totales. Usamos el primero (respeta cuotas).
        if unsafe { GetDiskFreeSpaceExW(ptr, Some(&mut libre), Some(&mut total), None) }.is_err() {
            // Lector de tarjetas vacío, por ejemplo: se omite sin fallar.
            continue;
        }
        let mut etiqueta = [0u16; 261];
        // GetVolumeInformationW (kernel32): nombre del volumen (etiqueta),
        // número de serie y sistema de archivos. Solo pedimos la etiqueta.
        let _ = unsafe { GetVolumeInformationW(ptr, Some(&mut etiqueta), None, None, None, None) };
        resultado.push(Unidad {
            raiz: raiz_texto,
            etiqueta: desde_utf16(&etiqueta),
            tipo,
            total,
            libre,
        });
    }
    Ok(resultado)
}

/// Convierte una hora Unix (ms, UTC) a texto en la hora local de este
/// equipo: `AAAA-MM-DD HH:MM:SS`. Respeta el horario de verano vigente en
/// esa fecha (no el de hoy).
pub fn hora_local(ms: i64) -> Option<String> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let v = u64::try_from(ms).ok()? * 10_000 + 116_444_736_000_000_000;
    let ft = FILETIME {
        dwLowDateTime: v as u32,
        dwHighDateTime: (v >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // FileTimeToSystemTime (kernel32): de "intervalos de 100 ns" a una fecha
    // con año, mes, día, hora... (todavía en UTC).
    unsafe { FileTimeToSystemTime(&ft, &mut utc) }.ok()?;
    // SystemTimeToTzSpecificLocalTime (kernel32): de UTC a la zona horaria
    // del equipo (None = la configurada en Windows), aplicando las reglas de
    // horario de verano que correspondían en esa fecha.
    unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.ok()?;
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    ))
}

/// Estado de la batería.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Bateria {
    /// Porcentaje de carga (0–100), si Windows lo sabe.
    pub porcentaje: Option<u8>,
    /// `Some(true)` si está enchufado a la corriente.
    pub enchufado: Option<bool>,
    pub cargando: bool,
    /// Ahorro de batería activado.
    pub ahorro: bool,
    /// Segundos de autonomía restantes estimados, si Windows los sabe.
    pub segundos_restantes: Option<u32>,
}

/// Lee el estado de la batería. `None` si el equipo no tiene batería.
pub fn bateria() -> Option<Bateria> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut st = SYSTEM_POWER_STATUS::default();
    // GetSystemPowerStatus (kernel32): estado de la alimentación. Los
    // valores "desconocido" vienen como 255 (bytes) o 0xFFFFFFFF.
    unsafe { GetSystemPowerStatus(&mut st) }.ok()?;
    // BatteryFlag: 128 = sin batería; 255 = estado desconocido.
    if st.BatteryFlag & 128 != 0 || st.BatteryFlag == 255 {
        return None;
    }
    Some(Bateria {
        porcentaje: (st.BatteryLifePercent <= 100).then_some(st.BatteryLifePercent),
        enchufado: match st.ACLineStatus {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        },
        cargando: st.BatteryFlag & 8 != 0,
        ahorro: st.SystemStatusFlag == 1,
        segundos_restantes: (st.BatteryLifeTime != u32::MAX).then_some(st.BatteryLifeTime),
    })
}

#[cfg(test)]
mod pruebas {
    #[test]
    fn hora_local_tiene_el_formato_esperado() {
        let t = super::hora_local(1_790_000_000_000).unwrap();
        assert_eq!(t.len(), 19);
        assert!(t.starts_with("2026-"));
        assert_eq!(super::hora_local(-1), None);
    }

    #[test]
    fn bateria_no_falla() {
        // Puede no haber batería (equipo de escritorio): solo se comprueba
        // que la lectura sea coherente cuando la hay.
        if let Some(b) = super::bateria() {
            assert!(b.porcentaje.is_none_or(|p| p <= 100));
        }
    }
}

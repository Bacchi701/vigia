//! Único módulo con `unsafe`: envuelve llamadas a la API de Windows en funciones seguras.
#![allow(unsafe_code)]

use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

pub struct Memoria {
    pub total: u64,
    pub disponible: u64,
}

/// Memoria física total y disponible.
pub fn memoria_sistema() -> windows::core::Result<Memoria> {
    let mut st = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // GlobalMemoryStatusEx (kernel32): llena MEMORYSTATUSEX con memoria física y virtual.
    unsafe { GlobalMemoryStatusEx(&mut st)? };
    Ok(Memoria {
        total: st.ullTotalPhys,
        disponible: st.ullAvailPhys,
    })
}

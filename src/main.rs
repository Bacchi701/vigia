//! Vigía: monitor local de Windows que solo observa.
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod win;

fn main() {
    match win::memoria_sistema() {
        Ok(m) => println!(
            "Memoria: {} MB usados de {} MB",
            (m.total - m.disponible) / 1_048_576,
            m.total / 1_048_576
        ),
        Err(e) => eprintln!("Error: {e}"),
    }
}

//! Notificación propia: se muestra de verdad durante ~1,5 s en la esquina
//! inferior derecha y se cierra sola.

use std::time::{Duration, Instant};
use vigia::win::notificacion::mostrar;

#[test]
fn se_muestra_se_cierra_sola_y_no_apila_mas_de_tres() {
    let inicio = Instant::now();
    let hilos: Vec<_> = (0..4)
        .map(|i| {
            std::thread::spawn(move || {
                // Escalonadas para que las tres primeras tomen lugar antes.
                std::thread::sleep(Duration::from_millis(150 * i));
                mostrar(
                    &format!("Prueba de Vigía {}", i + 1),
                    "Esta es una notificación de prueba; se cierra sola.",
                    1500,
                )
                .unwrap()
            })
        })
        .collect();
    let mostradas: Vec<bool> = hilos.into_iter().map(|h| h.join().unwrap()).collect();
    // Las tres primeras se muestran; la cuarta no tiene lugar.
    assert_eq!(mostradas, vec![true, true, true, false]);
    // Se cerraron solas, en poco más que su duración.
    assert!(inicio.elapsed() < Duration::from_secs(6));
    // Al cerrarse liberan su lugar: otra puede mostrarse.
    assert!(mostrar("Prueba de Vigía 5", "Última notificación de prueba.", 800).unwrap());
}

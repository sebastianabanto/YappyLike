//! Probe de aceptación de M2 (§6): imprime el texto seleccionado y confirma
//! que el portapapeles quedó intacto.
//!
//! Uso:
//! ```text
//! cargo run --example capture_probe
//! ```
//! Al arrancar, cambia a otra app (Notepad, navegador, VS Code…) y **selecciona
//! texto** antes de que termine la cuenta atrás. El probe enviará `Ctrl+C` al
//! foco activo, leerá la selección y restaurará tu portapapeles original.

use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

use yappylike_lib::capture::SelectionCapture;

fn main() {
    let cap = SelectionCapture::system();

    let before = cap.clipboard_text();
    println!("Portapapeles antes: {before:?}");
    println!("Cambia de ventana y SELECCIONA texto. Capturaré con Ctrl+C en unos segundos...");
    for i in (1..=5).rev() {
        print!("\r  capturando en {i}s… ");
        let _ = std::io::stdout().flush();
        sleep(Duration::from_secs(1));
    }
    println!("\rCapturando ahora.            ");

    match cap.capture_selected_text() {
        Some(text) => {
            println!("--- TEXTO CAPTURADO ({} chars) ---", text.chars().count());
            println!("{text}");
            println!("--- FIN ---");
        }
        None => println!("No se capturó texto (¿nada seleccionado?)."),
    }

    let after = cap.clipboard_text();
    println!("Portapapeles después: {after:?}");
    if before == after {
        println!("OK: el portapapeles quedó intacto.");
    } else {
        println!(
            "AVISO: el portapapeles cambió. Puede deberse a un formato no restaurable \
             (imagen/archivos) o a que copiaste algo durante la prueba."
        );
    }
}

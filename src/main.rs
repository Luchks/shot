mod capture;
mod geometry;
mod overlay;
mod selection;
mod wayland;

use std::path::PathBuf;
use std::process::ExitCode;

/// `my-screenshot [-o|--output RUTA]`
///
/// Sin argumentos: selección por teclado (cuadrícula 10x4 con las teclas
/// `1..0`, `q..p`, `a..;`, `z../`; Enter/Espacio,
/// Backspace, Escape) y copia el resultado al portapapeles con
/// `wl-copy`. Con `-o`/`--output`, además guarda una copia PNG en
/// `RUTA`.
fn parse_args() -> Result<Option<PathBuf>, String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => Ok(None),
        Some("-o") | Some("--output") => {
            let path = args
                .next()
                .ok_or_else(|| "falta la ruta después de -o/--output".to_string())?;
            Ok(Some(PathBuf::from(path)))
        }
        Some("-h") | Some("--help") => {
            println!("Uso: my-screenshot [-o|--output RUTA]");
            std::process::exit(0);
        }
        Some(other) => Err(format!("argumento no reconocido: {other}")),
    }
}

fn main() -> ExitCode {
    let save_path = match parse_args() {
        Ok(path) => path,
        Err(msg) => {
            eprintln!("my-screenshot: {msg}");
            eprintln!("Uso: my-screenshot [-o|--output RUTA]");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = wayland::run(save_path) {
        eprintln!("my-screenshot: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

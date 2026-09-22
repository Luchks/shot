
use std::fmt;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

/// Región de captura en coordenadas globales del compositor, en el mismo
/// formato que espera `grim -g`: `"X,Y WxH"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl CaptureRegion {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    /// Formatea la región en la sintaxis `"X,Y WxH"` que acepta `grim -g`.
    fn grim_geometry(&self) -> String {
        format!("{},{} {}x{}", self.x, self.y, self.width, self.height)
    }
}

impl fmt::Display for CaptureRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.grim_geometry())
    }
}

/// Errores propios de la captura, con el suficiente contexto para
/// mostrarle algo útil al usuario (p. ej. "grim no está instalado") sin
/// exponer directamente `std::io::Error`.
#[derive(Debug)]
pub enum CaptureError {
    /// No se pudo lanzar el proceso (binario ausente, sin permisos, etc.).
    Spawn { program: &'static str, source: io::Error },
    /// El proceso terminó pero devolvió un código de salida distinto de 0.
    NonZeroExit { program: &'static str, status: std::process::ExitStatus },
    /// Falló la escritura/lectura de un pipe entre `grim` y `wl-copy`.
    Pipe { detail: &'static str, source: io::Error },
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::Spawn { program, source } => {
                write!(f, "no se pudo ejecutar `{program}`: {source} (¿está instalado y en el PATH?)")
            }
            CaptureError::NonZeroExit { program, status } => {
                write!(f, "`{program}` terminó con {status}")
            }
            CaptureError::Pipe { detail, source } => {
                write!(f, "error de E/S entre grim y wl-copy ({detail}): {source}")
            }
        }
    }
}

impl std::error::Error for CaptureError {}

/// Copia la región capturada al portapapeles ejecutando
/// `grim -g "X,Y WxH" -` con la salida conectada directamente a la entrada
/// de `wl-copy --type image/png`, sin pasar por un archivo temporal ni
/// bufferizar la imagen completa en memoria del proceso.
pub fn copy_to_clipboard(region: CaptureRegion) -> Result<(), CaptureError> {
    let mut grim = Command::new("grim")
        .arg("-g")
        .arg(region.grim_geometry())
        .arg("-") // salida a stdout
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|source| CaptureError::Spawn { program: "grim", source })?;

    let grim_stdout = grim
        .stdout
        .take()
        .expect("grim fue lanzado con stdout piped, stdout debe existir");

    let mut wl_copy = Command::new("wl-copy")
        .arg("--type")
        .arg("image/png")
        .stdin(Stdio::from(grim_stdout))
        .spawn()
        .map_err(|source| CaptureError::Spawn { program: "wl-copy", source })?;

    let grim_status = grim
        .wait()
        .map_err(|source| CaptureError::Pipe { detail: "esperando a grim", source })?;
    if !grim_status.success() {
        return Err(CaptureError::NonZeroExit { program: "grim", status: grim_status });
    }

    let wl_copy_status = wl_copy
        .wait()
        .map_err(|source| CaptureError::Pipe { detail: "esperando a wl-copy", source })?;
    if !wl_copy_status.success() {
        return Err(CaptureError::NonZeroExit { program: "wl-copy", status: wl_copy_status });
    }

    Ok(())
}

/// Guarda la región capturada directamente en `path` con una segunda
/// invocación de `grim` (independiente de `copy_to_clipboard`). Se prefiere
/// esto a "interceptar" el PNG que ya viaja hacia `wl-copy`: mantiene cada
/// función responsable de un solo destino y evita tener que bufferizar la
/// imagen completa en memoria para poder escribirla dos veces.
pub fn save_to_path(region: CaptureRegion, path: &Path) -> Result<(), CaptureError> {
    let status = Command::new("grim")
        .arg("-g")
        .arg(region.grim_geometry())
        .arg(path)
        .status()
        .map_err(|source| CaptureError::Spawn { program: "grim", source })?;

    if !status.success() {
        return Err(CaptureError::NonZeroExit { program: "grim", status });
    }
    Ok(())
}

/// Punto de entrada único usado por `wayland.rs` al confirmar la Fase 2:
/// siempre copia al portapapeles y, si se indicó una ruta, además guarda
/// una copia en disco.
pub fn capture_and_copy(region: CaptureRegion, save_path: Option<&Path>) -> Result<(), CaptureError> {
    copy_to_clipboard(region)?;
    if let Some(path) = save_path {
        save_to_path(region, path)?;
        // Aviso por stderr, no por stdout: stdout se deja libre por si en
        // el futuro se quiere componer con otras herramientas de línea de
        // comandos.
        let _ = writeln!(io::stderr(), "Guardado en {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grim_geometry_formats_as_x_comma_y_space_wxh() {
        let r = CaptureRegion::new(100, 200, 300, 400);
        assert_eq!(r.grim_geometry(), "100,200 300x400");
    }

    #[test]
    fn grim_geometry_handles_negative_origin() {
        // Un monitor puede estar a la izquierda/arriba del origen global
        // en un layout multi-monitor; grim acepta coordenadas negativas.
        let r = CaptureRegion::new(-1920, 0, 1920, 1080);
        assert_eq!(r.grim_geometry(), "-1920,0 1920x1080");
    }

    #[test]
    fn display_matches_grim_geometry() {
        let r = CaptureRegion::new(0, 0, 10, 10);
        assert_eq!(r.to_string(), r.grim_geometry());
    }
}

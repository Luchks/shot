use std::fmt;

//use smithay_client_toolkit::shm::slot::{Buffer, CreateBufferError, CreatePoolError, SlotPool};
use smithay_client_toolkit::shm::slot::{Buffer, CreateBufferError, SlotPool};
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::CreatePoolError;



use wayland_client::protocol::wl_shm;

use crate::geometry::{Point, Quadrant, Rect};
use crate::selection::{Phase, SelectionState};

// ---------------------------------------------------------------------
// Paleta / tema del overlay
// ---------------------------------------------------------------------

/// Color en formato RGBA de 8 bits por canal, independiente del layout de
/// bytes en memoria (eso lo resuelve [`Canvas::set_pixel`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl Color {
    const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }
}

/// Negro al 40% de opacidad: fondo oscurecido sobre todo el monitor.
const BACKGROUND_TINT: Color = Color::rgba(0, 0, 0, 102);
/// Borde brillante cian alrededor del rectángulo activo.
const BORDER_COLOR: Color = Color::rgb(56, 226, 255);
/// Guías internas tenues (blanco con alpha bajo) que marcan los 4 cuadrantes.
const GUIDE_COLOR: Color = Color::rgba(255, 255, 255, 80);
/// Marca del Punto A fijado en Fase 2 (ámbar, bien distinguible del cian).
const POINT_A_COLOR: Color = Color::rgb(255, 190, 0);
/// Color de las etiquetas u/i/j/k.
const LABEL_COLOR: Color = Color::rgb(255, 255, 255);

const BORDER_THICKNESS: i32 = 3;
const GUIDE_THICKNESS: i32 = 1;
const POINT_A_RADIUS: i32 = 7;
const POINT_A_THICKNESS: i32 = 2;
/// Factor de escala de la fuente 8x8 embebida (8*4 = 32px de alto por letra).
const LABEL_SCALE: i32 = 4;

// ---------------------------------------------------------------------
// Fuente bitmap 8x8 embebida (solo u, i, j, k; ver nota en `glyph_for`)
// ---------------------------------------------------------------------

/// Glifo monoespaciado de 8x8. Cada `u8` es una fila; el bit más
/// significativo (bit 7) es la columna 0 (izquierda), el menos
/// significativo (bit 0) es la columna 7 (derecha).
struct Glyph8x8 {
    rows: [u8; 8],
}

const GLYPH_U: Glyph8x8 = Glyph8x8 {
    rows: [
        0b01100110,
        0b01100110,
        0b01100110,
        0b01100110,
        0b01100110,
        0b01100110,
        0b01100110,
        0b00111100,
    ],
};

const GLYPH_I: Glyph8x8 = Glyph8x8 {
    rows: [
        0b01111110,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b01111110,
    ],
};

const GLYPH_J: Glyph8x8 = Glyph8x8 {
    rows: [
        0b00011110,
        0b00001100,
        0b00001100,
        0b00001100,
        0b00001100,
        0b11001100,
        0b11001100,
        0b01111000,
    ],
};

const GLYPH_K: Glyph8x8 = Glyph8x8 {
    rows: [
        0b11000110,
        0b11001100,
        0b11011000,
        0b11110000,
        0b11011000,
        0b11001100,
        0b11000110,
        0b11000011,
    ],
};

/// Devuelve el glifo embebido para una tecla vim (`u`/`i`/`j`/`k`,
/// mayúscula o minúscula).
///
/// Solo cubre esas cuatro letras porque son las únicas que este overlay
/// necesita dibujar. El array de glifos está pensado para extenderse: añadir
/// una letra nueva es agregar una constante `Glyph8x8` más y un brazo a
/// este `match`, sin tocar el resto del renderizador.
fn glyph_for(key: char) -> Option<&'static Glyph8x8> {
    match key.to_ascii_uppercase() {
        'U' => Some(&GLYPH_U),
        'I' => Some(&GLYPH_I),
        'J' => Some(&GLYPH_J),
        'K' => Some(&GLYPH_K),
        _ => None,
    }
}

// ---------------------------------------------------------------------
// Canvas: primitivas de dibujo sobre el buffer de píxeles crudo
// ---------------------------------------------------------------------

/// Vista de dibujo sobre un buffer SHM ya mapeado en memoria.
///
/// No sabe nada de Wayland: opera sobre cualquier `&mut [u8]` con el
/// layout de `wl_shm::Format::Argb8888`, lo que la hace trivialmente
/// testeable sin una conexión Wayland real (ver los tests al final del
/// archivo).
struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: i32,
    height: i32,
    stride: i32,
}

impl<'a> Canvas<'a> {
    fn new(pixels: &'a mut [u8], width: i32, height: i32, stride: i32) -> Self {
        Self { pixels, width, height, stride }
    }

    /// Escribe un píxel. Fuera de límites es un no-op silencioso: el
    /// renderizador dibuja formas cuyas coordenadas pueden exceder el
    /// buffer (p. ej. un borde justo en el último píxel), y no queremos
    /// que eso entre en pánico.
    #[inline]
    fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.stride + x * 4) as usize;
        if offset + 4 > self.pixels.len() {
            return;
        }
        // wl_shm::Format::Argb8888 representa cada píxel como una palabra
        // de 32 bits 0xAARRGGBB. En memoria, en una máquina little-endian
        // (x86_64/aarch64, el caso estándar en Arch Linux), el byte menos
        // significativo va primero: B, G, R, A.
        self.pixels[offset] = color.b;
        self.pixels[offset + 1] = color.g;
        self.pixels[offset + 2] = color.r;
        self.pixels[offset + 3] = color.a;
    }

    fn fill(&mut self, color: Color) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.set_pixel(x, y, color);
            }
        }
    }

    fn draw_hline(&mut self, x0: i32, x1: i32, y: i32, thickness: i32, color: Color) {
        let thickness = thickness.max(1);
        for dy in 0..thickness {
            for x in x0..x1 {
                self.set_pixel(x, y + dy, color);
            }
        }
    }

    fn draw_vline(&mut self, y0: i32, y1: i32, x: i32, thickness: i32, color: Color) {
        let thickness = thickness.max(1);
        for dx in 0..thickness {
            for y in y0..y1 {
                self.set_pixel(x + dx, y, color);
            }
        }
    }

    fn draw_rect_outline(&mut self, rect: Rect, thickness: i32, color: Color) {
        let t = thickness.max(1);
        self.draw_hline(rect.x, rect.right(), rect.y, t, color); // arriba
        self.draw_hline(rect.x, rect.right(), rect.bottom() - t, t, color); // abajo
        self.draw_vline(rect.y, rect.bottom(), rect.x, t, color); // izquierda
        self.draw_vline(rect.y, rect.bottom(), rect.right() - t, t, color); // derecha
    }

    /// Marca en forma de cruz para el Punto A fijado en Fase 2.
    fn draw_cross(&mut self, center: Point, radius: i32, thickness: i32, color: Color) {
        let t = thickness.max(1);
        self.draw_hline(center.x - radius, center.x + radius, center.y - t / 2, t, color);
        self.draw_vline(center.y - radius, center.y + radius, center.x - t / 2, t, color);
    }

    fn draw_glyph(&mut self, glyph: &Glyph8x8, top_left: Point, scale: i32, color: Color) {
        let scale = scale.max(1);
        for (row, bits) in glyph.rows.iter().enumerate() {
            for col in 0..8u32 {
                let is_set = (bits >> (7 - col)) & 1 == 1;
                if !is_set {
                    continue;
                }
                let px = top_left.x + col as i32 * scale;
                let py = top_left.y + row as i32 * scale;
                for sy in 0..scale {
                    for sx in 0..scale {
                        self.set_pixel(px + sx, py + sy, color);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// Composición del frame a partir del SelectionState
// ---------------------------------------------------------------------

/// Dibuja las guías tenues que dividen `rect` en sus 4 cuadrantes.
/// No dibuja nada si `rect` ya no admite subdivisión (las guías dejarían
/// de tener sentido: no habría un u/i/j/k útil que resaltar).
fn draw_quadrant_guides(canvas: &mut Canvas, rect: Rect) {
    if !rect.can_subdivide() {
        return;
    }
    let mid_x = rect.x + rect.width / 2;
    let mid_y = rect.y + rect.height / 2;
    canvas.draw_vline(rect.y, rect.bottom(), mid_x, GUIDE_THICKNESS, GUIDE_COLOR);
    canvas.draw_hline(rect.x, rect.right(), mid_y, GUIDE_THICKNESS, GUIDE_COLOR);
}

/// Dibuja las etiquetas u/i/j/k centradas en cada uno de los 4 cuadrantes
/// de `rect`. Si un cuadrante resultante es demasiado pequeño para la
/// letra a la escala configurada, esa etiqueta se omite (evita solapes
/// ilegibles en rectángulos ya muy subdivididos).
fn draw_quadrant_labels(canvas: &mut Canvas, rect: Rect, scale: i32) {
    if !rect.can_subdivide() {
        return;
    }

    const QUADRANT_KEYS: [(Quadrant, char); 4] = [
        (Quadrant::TopLeft, 'U'),
        (Quadrant::TopRight, 'I'),
        (Quadrant::BottomLeft, 'J'),
        (Quadrant::BottomRight, 'K'),
    ];

    let glyph_px = 8 * scale;

    for (quadrant, key) in QUADRANT_KEYS {
        let sub = rect.subdivide(quadrant);
        if sub.width < glyph_px || sub.height < glyph_px {
            continue;
        }
        let glyph = glyph_for(key).expect("glifo interno faltante para una tecla vim válida");
        let center_x = sub.x + sub.width / 2;
        let center_y = sub.y + sub.height / 2;
        let top_left = Point::new(center_x - glyph_px / 2, center_y - glyph_px / 2);
        canvas.draw_glyph(glyph, top_left, scale, LABEL_COLOR);
    }
}

/// Pinta un frame completo del overlay directamente sobre `pixels`.
///
/// Función pura (sin estado propio ni dependencias de Wayland): dado el
/// mismo `selection`, siempre produce el mismo contenido. Esto es lo que
/// permite testearla sin una conexión Wayland real.
fn paint_frame(pixels: &mut [u8], width: i32, height: i32, stride: i32, selection: &SelectionState) {
    let mut canvas = Canvas::new(pixels, width, height, stride);

    // 1. Fondo semitransparente oscuro sobre todo el monitor.
    canvas.fill(BACKGROUND_TINT);

    let current = selection.current();

    // 2. Guías internas de los 4 cuadrantes del rectángulo activo.
    draw_quadrant_guides(&mut canvas, current);

    // 3. Etiquetas u/i/j/k en cada cuadrante.
    draw_quadrant_labels(&mut canvas, current, LABEL_SCALE);

    // 4. Borde resaltado del rectángulo activo (encima de las guías).
    canvas.draw_rect_outline(current, BORDER_THICKNESS, BORDER_COLOR);

    // 5. Marca del Punto A, solo visible en Fase 2.
    if selection.phase() == Phase::Second {
        if let Some(a) = selection.point_a() {
            canvas.draw_cross(a, POINT_A_RADIUS, POINT_A_THICKNESS, POINT_A_COLOR);
        }
    }
}

// ---------------------------------------------------------------------
// Overlay: gestión de buffers SHM (wl_shm) vía SlotPool de SCTK
// ---------------------------------------------------------------------

/// Errores propios de la creación de pool/buffer SHM, envueltos para no
/// filtrar directamente los tipos de `smithay_client_toolkit` a quien
/// use este módulo.
#[derive(Debug)]
pub enum OverlayError {
    CreatePool(CreatePoolError),
    CreateBuffer(CreateBufferError),
}

impl fmt::Display for OverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OverlayError::CreatePool(e) => write!(f, "no se pudo crear el pool SHM: {e}"),
            OverlayError::CreateBuffer(e) => write!(f, "no se pudo crear el buffer SHM: {e}"),
        }
    }
}

impl std::error::Error for OverlayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OverlayError::CreatePool(e) => Some(e),
            OverlayError::CreateBuffer(e) => Some(e),
        }
    }
}

impl From<CreatePoolError> for OverlayError {
    fn from(e: CreatePoolError) -> Self {
        OverlayError::CreatePool(e)
    }
}

impl From<CreateBufferError> for OverlayError {
    fn from(e: CreateBufferError) -> Self {
        OverlayError::CreateBuffer(e)
    }
}

/// Renderizador del overlay para un output (monitor) concreto.
///
/// Mantiene un [`SlotPool`] dimensionado para el tamaño lógico del
/// monitor. El tamaño del buffer nunca cambia entre frames — lo único
/// que cambia entre subdivisiones es el contenido que se pinta dentro de
/// ese buffer — así que no hay lógica de resize aquí.
pub struct Overlay {
    pool: SlotPool,
    width: i32,
    height: i32,
    stride: i32,
    format: wl_shm::Format,
}

impl Overlay {
    /// Crea el overlay para un monitor de `width`x`height` píxeles
    /// lógicos, reservando en el pool SHM espacio para dos frames
    /// completos (doble buffer): mientras el compositor todavía no ha
    /// liberado el buffer del frame anterior, `SlotPool` puede servir el
    /// siguiente sin tener que crecer el mmap ni bloquear.
    pub fn new(shm: &Shm, width: i32, height: i32) -> Result<Self, OverlayError> {
        assert!(width > 0 && height > 0, "dimensiones de overlay inválidas: {width}x{height}");

        let format = wl_shm::Format::Argb8888;
        let stride = width * 4;
        let single_frame_len = stride as usize * height as usize;
        let pool = SlotPool::new(single_frame_len * 2, shm)?;

        Ok(Self { pool, width, height, stride, format })
    }

    // Accesores públicos para completar la API de `Overlay`; el flujo
    // actual solo necesita `width`/`height` internamente al construir el
    // overlay, no después.
    #[allow(dead_code)]
    pub fn width(&self) -> i32 {
        self.width
    }

    #[allow(dead_code)]
    pub fn height(&self) -> i32 {
        self.height
    }

    /// Pinta un nuevo frame para `selection` y devuelve el [`Buffer`] SHM
    /// resultante, listo para que la capa de integración con Wayland lo
    /// adjunte a la superficie (`wl_surface.attach` + `damage_buffer` +
    /// `commit`) y lo descarte a continuación: la reutilización de la
    /// memoria subyacente la sigue gestionando `SlotPool` internamente a
    /// través de los eventos `wl_buffer::Release`, con independencia de
    /// cuánto tiempo se conserve este valor `Buffer` en el llamador.
    pub fn render(&mut self, selection: &SelectionState) -> Result<Buffer, OverlayError> {
        let (buffer, pixels) = self
            .pool
            .create_buffer(self.width, self.height, self.stride, self.format)?;

        paint_frame(pixels, self.width, self.height, self.stride, selection);

        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::selection::SelectionState;

    /// Lee de vuelta el color de un píxel ya escrito, para verificar el
    /// contenido pintado sin depender de una conexión Wayland real.
    fn pixel_at(pixels: &[u8], stride: i32, x: i32, y: i32) -> Color {
        let offset = (y * stride + x * 4) as usize;
        Color::rgba(
            pixels[offset + 2],
            pixels[offset + 1],
            pixels[offset],
            pixels[offset + 3],
        )
    }

    fn blank_buffer(width: i32, height: i32) -> (Vec<u8>, i32) {
        let stride = width * 4;
        (vec![0u8; (stride * height) as usize], stride)
    }

    #[test]
    fn set_pixel_uses_bgra_byte_order_for_argb8888() {
        let (mut buf, stride) = blank_buffer(4, 4);
        let mut canvas = Canvas::new(&mut buf, 4, 4, stride);
        canvas.set_pixel(1, 1, Color::rgba(10, 20, 30, 40));

        let offset = (1 * stride + 1 * 4) as usize;
        assert_eq!(buf[offset], 30); // B
        assert_eq!(buf[offset + 1], 20); // G
        assert_eq!(buf[offset + 2], 10); // R
        assert_eq!(buf[offset + 3], 40); // A
    }

    #[test]
    fn set_pixel_out_of_bounds_is_a_silent_no_op() {
        let (mut buf, stride) = blank_buffer(4, 4);
        let mut canvas = Canvas::new(&mut buf, 4, 4, stride);
        // No debe entrar en pánico ni corromper memoria adyacente.
        canvas.set_pixel(-1, 0, Color::rgb(255, 255, 255));
        canvas.set_pixel(0, -1, Color::rgb(255, 255, 255));
        canvas.set_pixel(4, 0, Color::rgb(255, 255, 255));
        canvas.set_pixel(0, 4, Color::rgb(255, 255, 255));
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn fill_sets_every_pixel_to_the_given_color() {
        let (mut buf, stride) = blank_buffer(3, 2);
        let mut canvas = Canvas::new(&mut buf, 3, 2, stride);
        canvas.fill(Color::rgba(0, 0, 0, 102));

        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(pixel_at(&buf, stride, x, y), Color::rgba(0, 0, 0, 102));
            }
        }
    }

    #[test]
    fn draw_rect_outline_only_touches_border_pixels() {
        let (mut buf, stride) = blank_buffer(10, 10);
        let mut canvas = Canvas::new(&mut buf, 10, 10, stride);
        let rect = Rect::new(2, 2, 6, 6);
        canvas.draw_rect_outline(rect, 1, Color::rgb(255, 255, 255));

        // Esquinas del borde: pintadas.
        assert_eq!(pixel_at(&buf, stride, 2, 2), Color::rgb(255, 255, 255));
        assert_eq!(pixel_at(&buf, stride, 7, 7), Color::rgb(255, 255, 255));
        // Centro del rectángulo: sin tocar.
        assert_eq!(pixel_at(&buf, stride, 5, 5), Color::rgba(0, 0, 0, 0));
    }

    #[test]
    fn quadrant_guides_are_skipped_when_rect_cannot_be_subdivided() {
        let (mut buf, stride) = blank_buffer(10, 10);
        let mut canvas = Canvas::new(&mut buf, 10, 10, stride);
        // 1x1: can_subdivide() es false.
        draw_quadrant_guides(&mut canvas, Rect::new(4, 4, 1, 1));
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn glyph_for_covers_exactly_the_four_vim_keys() {
        for key in ['u', 'U', 'i', 'I', 'j', 'J', 'k', 'K'] {
            assert!(glyph_for(key).is_some(), "falta glifo para '{key}'");
        }
        for key in ['a', 'z', '1', ' '] {
            assert!(glyph_for(key).is_none(), "no debería haber glifo para '{key}'");
        }
    }

    #[test]
    fn paint_frame_draws_point_a_marker_only_in_phase_two() {
        let monitor = Rect::new(0, 0, 64, 64);
        let (mut buf, stride) = blank_buffer(64, 64);

        // Fase 1: sin marca de Punto A todavía. No podemos comprobar esto
        // mirando un único píxel como (0,0): en Fase 1 `current` es el
        // monitor completo, así que el borde del rectángulo activo
        // (`draw_rect_outline`) también pinta esa esquina, y confundía esa
        // pintura con la marca del Punto A. Comprobamos en su lugar que
        // POINT_A_COLOR no aparece en ningún píxel del frame.
        let phase_one = SelectionState::new(monitor);
        paint_frame(&mut buf, 64, 64, stride, &phase_one);
        let has_point_a_marker = (0..64)
            .any(|y| (0..64).any(|x| pixel_at(&buf, stride, x, y) == POINT_A_COLOR));
        assert!(!has_point_a_marker, "no debería haber marca de Punto A en Fase 1");

        // Fase 2: confirmamos con el rectángulo completo -> A = (0,0).
        let mut phase_two = SelectionState::new(monitor);
        phase_two.confirm();
        let (mut buf2, stride2) = blank_buffer(64, 64);
        paint_frame(&mut buf2, 64, 64, stride2, &phase_two);

        // El brazo horizontal de la cruz en A=(0,0) pinta a la derecha
        // del centro (el buffer no tiene píxeles negativos que pintar).
        let marked = (0..POINT_A_RADIUS)
            .any(|dx| pixel_at(&buf2, stride2, dx, 0) == POINT_A_COLOR);
        assert!(marked, "se esperaba la marca del Punto A cerca de (0,0)");
    }

    #[test]
    fn paint_frame_is_pure_and_deterministic() {
        let monitor = Rect::new(0, 0, 32, 32);
        let selection = SelectionState::new(monitor);

        let (mut buf_a, stride) = blank_buffer(32, 32);
        let (mut buf_b, _) = blank_buffer(32, 32);

        paint_frame(&mut buf_a, 32, 32, stride, &selection);
        paint_frame(&mut buf_b, 32, 32, stride, &selection);

        assert_eq!(buf_a, buf_b);
    }
}

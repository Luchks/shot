//! Primitivas geométricas puras (sin dependencias de Wayland) usadas por el
//! motor de selección: puntos, rectángulos y la cuadrícula de 10x4 celdas
//! que corresponde a las cuatro filas del teclado (`1..0`, `q..p`, `a..;`,
//! `z../`).
//!
//! Todo aquí trabaja en coordenadas lógicas de un único output (monitor),
//! con origen arriba-izquierda, tal como las entrega wlr-layer-shell.

/// Un punto en coordenadas lógicas de pantalla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Número de columnas de la cuadrícula de selección (una por tecla de
/// cada fila del teclado: `1..0`, `q..p`, `a..;`, `z../`).
pub const GRID_COLS: i32 = 10;
/// Número de filas de la cuadrícula de selección (las 4 filas del teclado).
pub const GRID_ROWS: i32 = 4;

/// Una celda de la cuadrícula `GRID_COLS` x `GRID_ROWS` en la que se
/// subdivide un [`Rect`]. `col` va de `0` (izquierda) a `GRID_COLS - 1`;
/// `row` de `0` (arriba) a `GRID_ROWS - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    pub col: i32,
    pub row: i32,
}

impl Cell {
    /// # Panics
    /// Si `col`/`row` quedan fuera de la cuadrícula (error de programación).
    pub fn new(col: i32, row: i32) -> Self {
        assert!((0..GRID_COLS).contains(&col), "Cell::new: col fuera de rango ({col})");
        assert!((0..GRID_ROWS).contains(&row), "Cell::new: row fuera de rango ({row})");
        Self { col, row }
    }
}

/// Posición del corte `i` (de `0` a `parts`) al dividir el segmento
/// `[start, start + len)` en `parts` trozos lo más iguales posible.
/// Los trozos difieren como mucho en 1 píxel y juntos cubren exactamente
/// `len` píxeles, sin huecos ni solapes.
fn split_at(start: i32, len: i32, parts: i32, i: i32) -> i32 {
    start + ((len as i64 * i as i64) / parts as i64) as i32
}

/// Rectángulo en coordenadas lógicas: `(x, y)` es la esquina superior
/// izquierda; `width`/`height` son siempre `>= 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    /// Crea un rectángulo. `width` y `height` deben ser no negativos.
    ///
    /// # Panics
    /// Si `width < 0` o `height < 0`. Esto se considera un error de
    /// programación (bug en el motor de selección), no una condición
    /// esperada en tiempo de ejecución.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        assert!(width >= 0, "Rect::new: width negativo ({width})");
        assert!(height >= 0, "Rect::new: height negativo ({height})");
        Self { x, y, width, height }
    }

    /// Construye un rectángulo normalizado a partir de dos puntos
    /// arbitrarios (no requiere que `a` sea top-left ni que `b` sea
    /// bottom-right; el resultado siempre lo es).
    pub fn from_points(a: Point, b: Point) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let width = (a.x - b.x).abs();
        let height = (a.y - b.y).abs();
        Self { x, y, width, height }
    }

    pub fn top_left(&self) -> Point {
        Point::new(self.x, self.y)
    }

    // Se mantienen `top_right`/`bottom_left`/`contains_point` por simetría
    // de la API de `Rect` (ya se usan `top_left`/`bottom_right` en la
    // lógica real de selección) y porque son cubiertas por tests; el
    // binario en sí no las llama todavía.
    #[allow(dead_code)]
    pub fn top_right(&self) -> Point {
        Point::new(self.right(), self.y)
    }

    #[allow(dead_code)]
    pub fn bottom_left(&self) -> Point {
        Point::new(self.x, self.bottom())
    }

    pub fn bottom_right(&self) -> Point {
        Point::new(self.right(), self.bottom())
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// `true` si el rectángulo todavía admite una subdivisión útil, es
    /// decir, si al subdividirlo en la cuadrícula ninguna celda colapsaría
    /// a ancho o alto cero.
    pub fn can_subdivide(&self) -> bool {
        self.width >= GRID_COLS && self.height >= GRID_ROWS
    }

    #[allow(dead_code)]
    pub fn contains_point(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    /// Devuelve la celda `cell` de la cuadrícula `GRID_COLS` x `GRID_ROWS`
    /// en la que se divide este rectángulo.
    ///
    /// Cuando `width` (o `height`) no es múltiplo exacto del número de
    /// columnas (o filas), los píxeles sobrantes se reparten entre las
    /// celdas: unas quedan 1 píxel más anchas que otras, pero la suma de
    /// todas siempre es exactamente `self.width` (idem alturas), sin
    /// pérdida ni solapamiento de píxeles.
    ///
    /// No comprueba `can_subdivide`; si el rectángulo es más pequeño que
    /// la cuadrícula, alguna celda puede tener ancho o alto `0`. Es
    /// responsabilidad de la capa de selección invocar
    /// [`Rect::can_subdivide`] antes de llamar a este método.
    pub fn subdivide(&self, cell: Cell) -> Rect {
        let x0 = split_at(self.x, self.width, GRID_COLS, cell.col);
        let x1 = split_at(self.x, self.width, GRID_COLS, cell.col + 1);
        let y0 = split_at(self.y, self.height, GRID_ROWS, cell.row);
        let y1 = split_at(self.y, self.height, GRID_ROWS, cell.row + 1);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Coordenadas X de las `GRID_COLS - 1` líneas internas que separan
    /// las columnas (para dibujar las guías del overlay).
    pub fn column_boundaries(&self) -> impl Iterator<Item = i32> + '_ {
        (1..GRID_COLS).map(move |i| split_at(self.x, self.width, GRID_COLS, i))
    }

    /// Coordenadas Y de las `GRID_ROWS - 1` líneas internas que separan
    /// las filas.
    pub fn row_boundaries(&self) -> impl Iterator<Item = i32> + '_ {
        (1..GRID_ROWS).map(move |i| split_at(self.y, self.height, GRID_ROWS, i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_and_bottom_are_computed_from_origin_and_size() {
        let r = Rect::new(10, 20, 100, 50);
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 70);
    }

    #[test]
    fn corner_points_match_expected_coordinates() {
        let r = Rect::new(0, 0, 200, 100);
        assert_eq!(r.top_left(), Point::new(0, 0));
        assert_eq!(r.top_right(), Point::new(200, 0));
        assert_eq!(r.bottom_left(), Point::new(0, 100));
        assert_eq!(r.bottom_right(), Point::new(200, 100));
    }

    #[test]
    fn from_points_normalizes_regardless_of_order() {
        let a = Point::new(300, 250);
        let b = Point::new(100, 50);
        let r = Rect::from_points(a, b);
        assert_eq!(r, Rect::new(100, 50, 200, 200));

        // Orden inverso debe dar el mismo resultado.
        let r2 = Rect::from_points(b, a);
        assert_eq!(r, r2);
    }

    #[test]
    fn from_points_handles_degenerate_same_point() {
        let p = Point::new(42, 42);
        let r = Rect::from_points(p, p);
        assert_eq!(r, Rect::new(42, 42, 0, 0));
        assert!(r.is_empty());
    }

    #[test]
    fn subdivide_exact_multiple_gives_equal_cells() {
        // 1920 / 10 = 192, 1080 / 4 = 270.
        let r = Rect::new(0, 0, 1920, 1080);

        assert_eq!(r.subdivide(Cell::new(0, 0)), Rect::new(0, 0, 192, 270));
        assert_eq!(r.subdivide(Cell::new(9, 0)), Rect::new(1728, 0, 192, 270));
        assert_eq!(r.subdivide(Cell::new(0, 3)), Rect::new(0, 810, 192, 270));
        assert_eq!(r.subdivide(Cell::new(9, 3)), Rect::new(1728, 810, 192, 270));
        assert_eq!(r.subdivide(Cell::new(4, 2)), Rect::new(768, 540, 192, 270));
    }

    #[test]
    fn subdivide_respects_rect_origin() {
        let r = Rect::new(100, 50, 200, 40);
        assert_eq!(r.subdivide(Cell::new(0, 0)), Rect::new(100, 50, 20, 10));
        assert_eq!(r.subdivide(Cell::new(9, 3)), Rect::new(280, 80, 20, 10));
    }

    #[test]
    fn subdivide_non_multiple_covers_every_pixel_exactly_once() {
        // Ni 1366 es múltiplo de 10 ni 767 de 4.
        let r = Rect::new(7, 3, 1366, 767);

        // Las columnas de una fila son contiguas y suman el ancho total.
        let mut x = r.x;
        for col in 0..GRID_COLS {
            let c = r.subdivide(Cell::new(col, 0));
            assert_eq!(c.x, x, "hueco o solape antes de la columna {col}");
            assert!(c.width == 136 || c.width == 137);
            x = c.right();
        }
        assert_eq!(x, r.right());

        // Las filas de una columna son contiguas y suman el alto total.
        let mut y = r.y;
        for row in 0..GRID_ROWS {
            let c = r.subdivide(Cell::new(0, row));
            assert_eq!(c.y, y, "hueco o solape antes de la fila {row}");
            assert!(c.height == 191 || c.height == 192);
            y = c.bottom();
        }
        assert_eq!(y, r.bottom());
    }

    #[test]
    fn boundaries_match_cell_edges() {
        let r = Rect::new(0, 0, 1366, 767);
        let cols: Vec<i32> = r.column_boundaries().collect();
        let rows: Vec<i32> = r.row_boundaries().collect();
        assert_eq!(cols.len(), (GRID_COLS - 1) as usize);
        assert_eq!(rows.len(), (GRID_ROWS - 1) as usize);
        for (i, x) in cols.iter().enumerate() {
            assert_eq!(*x, r.subdivide(Cell::new(i as i32 + 1, 0)).x);
        }
        for (i, y) in rows.iter().enumerate() {
            assert_eq!(*y, r.subdivide(Cell::new(0, i as i32 + 1)).y);
        }
    }

    #[test]
    fn subdivide_is_recursive_and_stable_across_many_levels() {
        let mut r = Rect::new(0, 0, 1920, 1080);
        while r.can_subdivide() {
            r = r.subdivide(Cell::new(GRID_COLS - 1, GRID_ROWS - 1));
        }
        // Tras subdividir hasta el límite el rectángulo sigue siendo
        // válido (no vacío, contenido en el monitor original).
        assert!(r.width > 0 && r.height > 0);
        assert!(r.right() == 1920 && r.bottom() == 1080);
    }

    #[test]
    fn can_subdivide_requires_one_pixel_per_cell() {
        assert!(Rect::new(0, 0, GRID_COLS, GRID_ROWS).can_subdivide());
        assert!(!Rect::new(0, 0, GRID_COLS - 1, 100).can_subdivide());
        assert!(!Rect::new(0, 0, 100, GRID_ROWS - 1).can_subdivide());
        assert!(!Rect::new(0, 0, 1, 1).can_subdivide());
    }

    #[test]
    #[should_panic(expected = "col fuera de rango")]
    fn cell_new_panics_out_of_range() {
        Cell::new(GRID_COLS, 0);
    }

    #[test]
    fn contains_point_includes_edges() {
        let r = Rect::new(10, 10, 100, 100);
        assert!(r.contains_point(Point::new(10, 10))); // esquina sup-izq
        assert!(r.contains_point(Point::new(110, 110))); // esquina inf-der
        assert!(!r.contains_point(Point::new(9, 10)));
        assert!(!r.contains_point(Point::new(10, 111)));
    }

    #[test]
    #[should_panic(expected = "width negativo")]
    fn new_panics_on_negative_width() {
        Rect::new(0, 0, -1, 10);
    }
}

//! Primitivas geométricas puras (sin dependencias de Wayland) usadas por el
//! motor de selección: puntos, rectángulos y la noción de "cuadrante" que
//! representa las cuatro teclas estilo vim (u/i/j/k).
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

/// Los cuatro cuadrantes en los que se puede subdividir un [`Rect`],
/// mapeados 1:1 a las teclas u/i/j/k.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quadrant {
    /// Tecla 'u'
    TopLeft,
    /// Tecla 'i'
    TopRight,
    /// Tecla 'j'
    BottomLeft,
    /// Tecla 'k'
    BottomRight,
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
    /// decir, si al subdividir ningún cuadrante resultante colapsaría a
    /// ancho o alto cero.
    pub fn can_subdivide(&self) -> bool {
        self.width > 1 && self.height > 1
    }

    #[allow(dead_code)]
    pub fn contains_point(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    /// Subdivide el rectángulo en el cuadrante indicado.
    ///
    /// Cuando `width` o `height` son impares, el píxel sobrante se asigna
    /// siempre a la mitad derecha/inferior, de modo que
    /// `top.width + bottom... ` — más precisamente: la suma de anchos de
    /// las dos mitades horizontales siempre es exactamente `self.width`
    /// (idem para alturas), sin pérdida ni solapamiento de píxeles.
    ///
    /// No comprueba `can_subdivide`; si `width` o `height` es `<= 1`,
    /// alguno de los cuadrantes resultantes puede tener ancho o alto `0`.
    /// Es responsabilidad de la capa de selección invocar
    /// [`Rect::can_subdivide`] antes de llamar a este método.
    pub fn subdivide(&self, quadrant: Quadrant) -> Rect {
        let half_w = self.width / 2;
        let rem_w = self.width - half_w;
        let half_h = self.height / 2;
        let rem_h = self.height - half_h;

        match quadrant {
            Quadrant::TopLeft => Rect::new(self.x, self.y, half_w, half_h),
            Quadrant::TopRight => Rect::new(self.x + half_w, self.y, rem_w, half_h),
            Quadrant::BottomLeft => Rect::new(self.x, self.y + half_h, half_w, rem_h),
            Quadrant::BottomRight => {
                Rect::new(self.x + half_w, self.y + half_h, rem_w, rem_h)
            }
        }
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
    fn subdivide_even_dimensions_splits_exactly_in_half() {
        let r = Rect::new(0, 0, 200, 100);

        assert_eq!(r.subdivide(Quadrant::TopLeft), Rect::new(0, 0, 100, 50));
        assert_eq!(r.subdivide(Quadrant::TopRight), Rect::new(100, 0, 100, 50));
        assert_eq!(r.subdivide(Quadrant::BottomLeft), Rect::new(0, 50, 100, 50));
        assert_eq!(
            r.subdivide(Quadrant::BottomRight),
            Rect::new(100, 50, 100, 50)
        );
    }

    #[test]
    fn subdivide_odd_dimensions_gives_remainder_to_right_and_bottom() {
        // 201x101: mitad izquierda/superior = 100, derecha/inferior = 101.
        let r = Rect::new(0, 0, 201, 101);

        let tl = r.subdivide(Quadrant::TopLeft);
        let tr = r.subdivide(Quadrant::TopRight);
        let bl = r.subdivide(Quadrant::BottomLeft);
        let br = r.subdivide(Quadrant::BottomRight);

        assert_eq!(tl, Rect::new(0, 0, 100, 50));
        assert_eq!(tr, Rect::new(100, 0, 101, 50));
        assert_eq!(bl, Rect::new(0, 50, 100, 51));
        assert_eq!(br, Rect::new(100, 50, 101, 51));

        // Invariante: ninguna franja pierde ni gana píxeles.
        assert_eq!(tl.width + tr.width, r.width);
        assert_eq!(tl.height + bl.height, r.height);
    }

    #[test]
    fn subdivide_is_recursive_and_stable_across_many_levels() {
        let mut r = Rect::new(0, 0, 1920, 1080);
        for _ in 0..8 {
            r = r.subdivide(Quadrant::BottomRight);
        }
        // Tras 8 subdivisiones sucesivas el rectángulo sigue siendo válido
        // (no negativo, contenido en el monitor original).
        assert!(r.width >= 0 && r.height >= 0);
        assert!(r.right() <= 1920 && r.bottom() <= 1080);
    }

    #[test]
    fn can_subdivide_is_false_at_minimum_size() {
        assert!(!Rect::new(0, 0, 1, 1).can_subdivide());
        assert!(!Rect::new(0, 0, 1, 50).can_subdivide());
        assert!(!Rect::new(0, 0, 50, 1).can_subdivide());
        assert!(Rect::new(0, 0, 2, 2).can_subdivide());
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

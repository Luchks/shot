use crate::geometry::{Cell, Point, Rect, GRID_COLS, GRID_ROWS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    First,
    Second,
}

pub struct SelectionState {
    monitor: Rect,
    current: Rect,
    point_a: Option<Point>,
    result: Option<Rect>,
    phase: Phase,
    history: Vec<Rect>,
}

/// Disposición de teclas de la cuadrícula (teclado inglés US), fila por
/// fila de arriba abajo. La tecla en `KEY_GRID[row][col]` selecciona la
/// celda `Cell { col, row }`.
pub const KEY_GRID: [[char; GRID_COLS as usize]; GRID_ROWS as usize] = [
    ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'],
    ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'],
    ['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';'],
    ['z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/'],
];

/// Traduce una tecla a su celda de la cuadrícula. Las letras no
/// distinguen mayúsculas (Bloq Mayús activado sigue funcionando).
pub fn cell_from_key(key: char) -> Option<Cell> {
    let key = key.to_ascii_lowercase();
    KEY_GRID.iter().enumerate().find_map(|(row, keys)| {
        keys.iter()
            .position(|&k| k == key)
            .map(|col| Cell::new(col as i32, row as i32))
    })
}

/// Tecla asociada a una celda (para dibujar las etiquetas del overlay).
pub fn key_for_cell(cell: Cell) -> char {
    KEY_GRID[cell.row as usize][cell.col as usize]
}

impl SelectionState {
    pub fn new(monitor: Rect) -> Self {
        Self {
            monitor,
            phase: Phase::First,
            current: monitor,
            point_a: None,
            result: None,
            history: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn monitor(&self) -> Rect {
        self.monitor
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn current(&self) -> Rect {
        self.current
    }

    pub fn point_a(&self) -> Option<Point> {
        self.point_a
    }

    /// `Some(rect)` una vez que ambas fases se han confirmado.
    pub fn result(&self) -> Option<Rect> {
        self.result
    }

    pub fn is_finished(&self) -> bool {
        self.result.is_some()
    }

    /// Aplica una tecla de la cuadrícula: subdivide `current` y se queda
    /// con la celda indicada.
    ///
    /// No hace nada (devuelve `false`) si:
    /// - la selección ya terminó (`is_finished()`), o
    /// - `current` no admite más subdivisión ([`Rect::can_subdivide`]).
    pub fn subdivide(&mut self, cell: Cell) -> bool {
        if self.is_finished() {
            return false;
        }
        if !self.current.can_subdivide() {
            return false;
        }
        self.history.push(self.current);
        self.current = self.current.subdivide(cell);
        true
    }

    /// Aplica `Enter`/`Space`.
    ///
    /// - En Fase 1: fija el Punto A como la esquina superior izquierda de
    ///   `current`, reinicia `current` al rectángulo `A -> esquina
    ///   inferior derecha del monitor`, limpia el historial de undo y
    ///   pasa a Fase 2.
    /// - En Fase 2: fija el Punto B como la esquina inferior derecha de
    ///   `current`, calcula la selección final `A..B` normalizada y
    ///   marca el proceso como terminado.
    ///
    /// No hace nada si la selección ya había terminado.
    pub fn confirm(&mut self) -> bool {
        if self.is_finished() {
            return false;
        }

        match self.phase {
            Phase::First => {
                let a = self.current.top_left();
                self.point_a = Some(a);

                let bottom_right = self.monitor.bottom_right();
                self.current = Rect::from_points(a, bottom_right);
                self.history.clear();
                self.phase = Phase::Second;
                true
            }
            Phase::Second => {
                let b = self.current.bottom_right();
                let a = self
                    .point_a
                    .expect("invariante violada: Fase 2 sin Punto A fijado");
                self.result = Some(Rect::from_points(a, b));
                true
            }
        }
    }

    /// Aplica `Backspace`: deshace exactamente un nivel de subdivisión
    /// dentro de la fase activa. No cruza de Fase 2 a Fase 1.
    ///
    /// Devuelve `false` (sin efecto) si:
    /// - la selección ya terminó, o
    /// - no hay nada que deshacer en la fase activa (estamos en el
    ///   estado inicial de la fase: monitor completo en Fase 1, o
    ///   `A -> esquina inferior derecha` recién entrado en Fase 2).
    pub fn undo(&mut self) -> bool {
        if self.is_finished() {
            return false;
        }
        match self.history.pop() {
            Some(prev) => {
                self.current = prev;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Cell, Point, Rect};

    fn monitor() -> Rect {
        Rect::new(0, 0, 1920, 1080)
    }

    #[test]
    fn starts_in_phase_one_with_full_monitor_active() {
        let s = SelectionState::new(monitor());
        assert_eq!(s.phase(), Phase::First);
        assert_eq!(s.current(), monitor());
        assert_eq!(s.point_a(), None);
        assert!(!s.is_finished());
    }

    #[test]
    fn cell_from_key_maps_the_four_keyboard_rows() {
        assert_eq!(cell_from_key('1'), Some(Cell::new(0, 0)));
        assert_eq!(cell_from_key('0'), Some(Cell::new(9, 0)));
        assert_eq!(cell_from_key('q'), Some(Cell::new(0, 1)));
        assert_eq!(cell_from_key('p'), Some(Cell::new(9, 1)));
        assert_eq!(cell_from_key('a'), Some(Cell::new(0, 2)));
        assert_eq!(cell_from_key(';'), Some(Cell::new(9, 2)));
        assert_eq!(cell_from_key('z'), Some(Cell::new(0, 3)));
        assert_eq!(cell_from_key('/'), Some(Cell::new(9, 3)));
        assert_eq!(cell_from_key('g'), Some(Cell::new(4, 2)));
    }

    #[test]
    fn cell_from_key_is_case_insensitive_and_rejects_other_keys() {
        assert_eq!(cell_from_key('Q'), cell_from_key('q'));
        assert_eq!(cell_from_key('M'), cell_from_key('m'));
        for key in ['-', '=', '[', ']', '\\', '\'', '`', ' ', ':', '<'] {
            assert_eq!(cell_from_key(key), None, "'{key}' no debería ser una celda");
        }
    }

    #[test]
    fn key_grid_has_forty_distinct_keys_and_round_trips() {
        let mut seen = std::collections::HashSet::new();
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let cell = Cell::new(col, row);
                let key = key_for_cell(cell);
                assert!(seen.insert(key), "tecla repetida: '{key}'");
                assert_eq!(cell_from_key(key), Some(cell));
            }
        }
        assert_eq!(seen.len(), 40);
    }

    #[test]
    fn subdivide_narrows_current_rect_recursively() {
        let mut s = SelectionState::new(monitor());

        assert!(s.subdivide(Cell::new(0, 0))); // '1'
        assert_eq!(s.current(), Rect::new(0, 0, 192, 270));

        assert!(s.subdivide(Cell::new(9, 3))); // '/'
        assert_eq!(s.current(), Rect::new(172, 202, 20, 68));
    }

    #[test]
    fn confirming_phase_one_sets_point_a_and_starts_phase_two() {
        let mut s = SelectionState::new(monitor());
        s.subdivide(Cell::new(2, 1)); // 'e' -> current = (384,270,192,270)
        s.subdivide(Cell::new(5, 2)); // 'h' -> current = (480,405,19,67)

        assert!(s.confirm());

        assert_eq!(s.phase(), Phase::Second);
        assert_eq!(s.point_a(), Some(Point::new(480, 405)));
        // Fase 2 empieza en A -> esquina inferior derecha del monitor.
        assert_eq!(s.current(), Rect::new(480, 405, 1440, 675));
        assert!(!s.is_finished());
    }

    #[test]
    fn confirm_in_phase_one_without_any_subdivision_uses_full_monitor_as_a() {
        let mut s = SelectionState::new(monitor());
        s.confirm();

        assert_eq!(s.point_a(), Some(Point::new(0, 0)));
        assert_eq!(s.current(), monitor());
    }

    #[test]
    fn full_two_phase_flow_produces_expected_final_rect() {
        let mut s = SelectionState::new(monitor());

        // Fase 1: celda 'w' -> (192,270,192,270); A = (192,270).
        s.subdivide(Cell::new(1, 1));
        s.confirm(); // Fase 2 activa sobre (192,270)-(1920,1080) = 1728x810

        // Fase 2: celda 'b' de esa subárea -> (883,877,173,203);
        // B = su esquina inf-der = (1056,1080).
        s.subdivide(Cell::new(4, 3));
        s.confirm();

        assert!(s.is_finished());
        assert_eq!(s.result(), Some(Rect::new(192, 270, 864, 810)));
    }

    #[test]
    fn undo_restores_the_rect_before_the_last_subdivision() {
        let mut s = SelectionState::new(monitor());
        let after_first = {
            s.subdivide(Cell::new(0, 0));
            s.current()
        };
        s.subdivide(Cell::new(9, 3));

        assert!(s.undo());
        assert_eq!(s.current(), after_first);
    }

    #[test]
    fn undo_at_start_of_phase_has_no_effect() {
        let mut s = SelectionState::new(monitor());
        // Nada que deshacer al inicio de la Fase 1.
        assert!(!s.undo());
        assert_eq!(s.current(), monitor());

        s.subdivide(Cell::new(0, 0));
        s.confirm(); // entra a Fase 2, historial se limpia.

        // Nada que deshacer al inicio de la Fase 2 (por diseño, no cruza
        // a Fase 1).
        assert!(!s.undo());
        assert_eq!(s.phase(), Phase::Second);
    }

    #[test]
    fn multiple_undo_calls_walk_back_multiple_levels_in_order() {
        let mut s = SelectionState::new(monitor());
        let r0 = s.current();
        s.subdivide(Cell::new(0, 0));
        let r1 = s.current();
        s.subdivide(Cell::new(9, 3));

        assert!(s.undo());
        assert_eq!(s.current(), r1);
        assert!(s.undo());
        assert_eq!(s.current(), r0);
        assert!(!s.undo());
    }

    #[test]
    fn subdivide_and_confirm_are_no_ops_once_finished() {
        let mut s = SelectionState::new(monitor());
        s.confirm(); // fija A, entra a Fase 2
        s.confirm(); // fija B, termina

        assert!(s.is_finished());
        let result_before = s.result();

        assert!(!s.subdivide(Cell::new(0, 0)));
        assert!(!s.confirm());
        assert!(!s.undo());
        assert_eq!(s.result(), result_before);
    }

    #[test]
    fn subdivide_ignored_when_current_rect_cannot_be_subdivided_further() {
        let tiny_monitor = Rect::new(0, 0, 1, 1);
        let mut s = SelectionState::new(tiny_monitor);
        assert!(!s.subdivide(Cell::new(0, 0)));
        assert_eq!(s.current(), tiny_monitor);
    }
}

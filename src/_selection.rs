use crate::geometry::{Point, Quadrant, Rect};

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

pub fn quadrant_from_key(key: char) -> Option<Quadrant> {
    match key.to_ascii_lowercase() {
        'u' => Some(Quadrant::TopLeft),
        'i' => Some(Quadrant::TopRight),
        'j' => Some(Quadrant::BottomLeft),
        'k' => Some(Quadrant::BottomRight),
        _ => None,
    }
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

    /// Aplica u/i/j/k: subdivide `current` en el cuadrante indicado.
    ///
    /// No hace nada (devuelve `false`) si:
    /// - la selección ya terminó (`is_finished()`), o
    /// - `current` no admite más subdivisión ([`Rect::can_subdivide`]).
    pub fn subdivide(&mut self, quadrant: Quadrant) -> bool {
        if self.is_finished() {
            return false;
        }
        if !self.current.can_subdivide() {
            return false;
        }
        self.history.push(self.current);
        self.current = self.current.subdivide(quadrant);
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
    use crate::geometry::{Point,Quadrant,Rect};

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
    fn quadrant_from_key_maps_vim_keys_case_insensitively() {
        assert_eq!(quadrant_from_key('u'), Some(Quadrant::TopLeft));
        assert_eq!(quadrant_from_key('U'), Some(Quadrant::TopLeft));
        assert_eq!(quadrant_from_key('i'), Some(Quadrant::TopRight));
        assert_eq!(quadrant_from_key('j'), Some(Quadrant::BottomLeft));
        assert_eq!(quadrant_from_key('k'), Some(Quadrant::BottomRight));
        assert_eq!(quadrant_from_key('x'), None);
    }

    #[test]
    fn subdivide_narrows_current_rect_recursively() {
        let mut s = SelectionState::new(monitor());

        assert!(s.subdivide(Quadrant::TopLeft));
        assert_eq!(s.current(), Rect::new(0, 0, 960, 540));

        assert!(s.subdivide(Quadrant::BottomRight));
        assert_eq!(s.current(), Rect::new(480, 270, 480, 270));
    }

    #[test]
    fn confirming_phase_one_sets_point_a_and_starts_phase_two() {
        let mut s = SelectionState::new(monitor());
        s.subdivide(Quadrant::TopLeft); // current = (0,0,960,540)
        s.subdivide(Quadrant::BottomRight); // current = (480,270,480,270)

        assert!(s.confirm());

        assert_eq!(s.phase(), Phase::Second);
        assert_eq!(s.point_a(), Some(Point::new(480, 270)));
        // Fase 2 empieza en A -> esquina inferior derecha del monitor.
        assert_eq!(s.current(), Rect::new(480, 270, 1440, 810));
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

        // Fase 1: nos quedamos con el cuadrante superior izquierdo.
        s.subdivide(Quadrant::TopLeft); // (0,0,960,540)
        s.confirm(); // A = (0,0); Fase 2 activa sobre (0,0,1920,1080)

        // Fase 2: nos quedamos con el cuadrante superior izquierdo de esa
        // subárea, es decir B = esquina inf-der de (0,0,960,540) = (960,540).
        s.subdivide(Quadrant::TopLeft);
        s.confirm();

        assert!(s.is_finished());
        assert_eq!(s.result(), Some(Rect::new(0, 0, 960, 540)));
    }

    #[test]
    fn undo_restores_the_rect_before_the_last_subdivision() {
        let mut s = SelectionState::new(monitor());
        let after_first = {
            s.subdivide(Quadrant::TopLeft);
            s.current()
        };
        s.subdivide(Quadrant::BottomRight);

        assert!(s.undo());
        assert_eq!(s.current(), after_first);
    }

    #[test]
    fn undo_at_start_of_phase_has_no_effect() {
        let mut s = SelectionState::new(monitor());
        // Nada que deshacer al inicio de la Fase 1.
        assert!(!s.undo());
        assert_eq!(s.current(), monitor());

        s.subdivide(Quadrant::TopLeft);
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
        s.subdivide(Quadrant::TopLeft);
        let r1 = s.current();
        s.subdivide(Quadrant::BottomRight);

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

        assert!(!s.subdivide(Quadrant::TopLeft));
        assert!(!s.confirm());
        assert!(!s.undo());
        assert_eq!(s.result(), result_before);
    }

    #[test]
    fn subdivide_ignored_when_current_rect_cannot_be_subdivided_further() {
        let tiny_monitor = Rect::new(0, 0, 1, 1);
        let mut s = SelectionState::new(tiny_monitor);
        assert!(!s.subdivide(Quadrant::TopLeft));
        assert_eq!(s.current(), tiny_monitor);
    }
}

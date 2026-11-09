
use std::path::PathBuf;
use std::process::Command;

use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_surface},
    Connection, QueueHandle,
};

use crate::capture::{self, CaptureRegion};
use crate::geometry::Rect;
use crate::overlay::Overlay;
use crate::selection::{cell_from_key, SelectionState};

/// Valores de keysym X11 (estables, heredados por xkbcommon) para las
/// teclas de control que no dependen de la disposición del teclado. Se
/// comparan contra el valor crudo del keysym en vez de depender de
/// constantes con nombre de una versión concreta de la crate
/// `xkbcommon`, lo que hace esta parte robusta a cambios de esa
/// dependencia.
mod keysyms {
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const SPACE: u32 = 0x0020;
    pub const BACKSPACE: u32 = 0xff08;
    pub const ESCAPE: u32 = 0xff1b;
}

/// Nombre (`wl_output` `name`, p. ej. "DP-1") del monitor que Hyprland
/// reporta como enfocado en el momento del lanzamiento, obtenido vía
/// `hyprctl` (IPC específico de Hyprland: no hay protocolo Wayland
/// estándar para "cuál output tiene el foco" desde un cliente que
/// todavía no tiene ninguna superficie). Si `hyprctl` no está disponible
/// o falla, se cae de vuelta al primer output que el compositor anuncie.
fn hyprctl_focused_monitor_name() -> Option<String> {
    let output = Command::new("hyprctl").args(["monitors", "-j"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_focused_monitor_name(&stdout)
}

/// Extrae el campo `"name"` del primer objeto con `"focused": true` del
/// JSON de `hyprctl monitors -j`. Se hace con búsqueda de texto en vez
/// de tirar de una crate de parseo JSON completa para leer un booleano y
/// una cadena de un formato que Hyprland mantiene estable.
fn parse_focused_monitor_name(json: &str) -> Option<String> {
    for monitor_obj in json.split("},") {
        let is_focused =
            monitor_obj.contains("\"focused\": true") || monitor_obj.contains("\"focused\":true");
        if !is_focused {
            continue;
        }
        let name_pos = monitor_obj.find("\"name\":")?;
        let after = &monitor_obj[name_pos + "\"name\":".len()..];
        let start = after.find('"')? + 1;
        let end = start + after[start..].find('"')?;
        return Some(after[start..end].to_string());
    }
    None
}

enum Outcome {
    Cancelled,
    Captured(CaptureRegion),
}

/// Estado completo de la aplicación, compartido por todos los `Handler`
/// que registran los macros `delegate_*!` más abajo.
struct AppState {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    seat_state: SeatState,

    keyboard: Option<wl_keyboard::WlKeyboard>,

    layer: LayerSurface,
    overlay: Option<Overlay>,
    selection: SelectionState,
    /// Posición lógica del output objetivo en el layout global del
    /// compositor. Necesaria para traducir el rectángulo final (en
    /// coordenadas locales del output) a la geometría global que
    /// `grim -g` espera.
    output_logical_position: (i32, i32),

    save_path: Option<PathBuf>,

    /// `true` tras el primer `configure` de la layer surface: hasta
    /// entonces no hay buffer que adjuntar.
    configured: bool,
    /// `Some` cuando el usuario canceló (Escape) o confirmó (Fase 2 +
    /// Enter/Espacio) y ya no queda nada más que dibujar.
    finished: Option<Outcome>,
}

/// Punto de entrada del módulo: conecta, negocia todos los globals,
/// localiza el monitor enfocado, crea la capa de overlay y corre el
/// bucle de eventos hasta que el usuario confirma o cancela.
///
/// `save_path`: si se pasa, además de copiar al portapapeles se guarda
/// una copia en esa ruta (ver `capture::capture_and_copy`).
pub fn run(save_path: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init::<AppState>(&conn)?;
    let qh = event_queue.handle();

    let compositor_state =
        CompositorState::bind(&globals, &qh).expect("wl_compositor no anunciado por el compositor");
    let layer_shell = LayerShell::bind(&globals, &qh)
        .expect("zwlr_layer_shell_v1 no anunciado (¿el compositor no es wlroots/Hyprland?)");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm no anunciado por el compositor");
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);
    let registry_state = RegistryState::new(&globals);

    let surface = compositor_state.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("my-screenshot"),
        None, // usar el output que el compositor asigne por defecto (foco actual del teclado)
    );
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0); // el compositor decide el tamaño en el primer configure
    layer.commit();

    let mut state = AppState {
        registry_state,
        output_state,
        shm,
        seat_state,
        keyboard: None,
        layer,
        overlay: None,
        selection: SelectionState::new(Rect::new(0, 0, 1, 1)), // reemplazado en el primer configure
        output_logical_position: (0, 0),
        save_path,
        configured: false,
        finished: None,
    };

    // Roundtrip: procesa los eventos wl_output::geometry/mode/name/scale
    // de todos los outputs y el primer configure de la layer surface
    // antes de entrar al bucle principal.
    event_queue.roundtrip(&mut state)?;

    if let Some(name) = hyprctl_focused_monitor_name() {
        state.output_logical_position = state
            .output_state
            .outputs()
            .filter_map(|o| state.output_state.info(&o))
            .find(|info| info.name.as_deref() == Some(name.as_str()))
            .and_then(|info| info.logical_position)
            .unwrap_or((0, 0));
    } else {
        state.output_logical_position = state
            .output_state
            .outputs()
            .next()
            .and_then(|o| state.output_state.info(&o))
            .and_then(|info| info.logical_position)
            .unwrap_or((0, 0));
    }

    let mut event_loop: EventLoop<AppState> = EventLoop::try_new()?;
    WaylandSource::new(conn, event_queue).insert(event_loop.handle())?;

    loop {
        event_loop.dispatch(std::time::Duration::from_millis(16), &mut state)?;

        if let Some(outcome) = state.finished.take() {
            // Desmapear la superficie (adjuntar un buffer nulo) ANTES de
            // invocar grim: mientras el overlay siga visible, la captura
            // se llevaría por delante nuestro propio tinte
            // semitransparente y el borde cian. `layerrule noanim` (ver
            // instrucciones de instalación) evita además que quede un
            // frame de animación de cierre visible en la captura.
            state.layer.wl_surface().attach(None, 0, 0);
            state.layer.wl_surface().commit();
            // Un dispatch adicional deja que el compositor procese el
            // desmapeo antes de que grim tome la captura.
            let _ = event_loop.dispatch(std::time::Duration::from_millis(50), &mut state);

            match outcome {
                Outcome::Cancelled => {}
                Outcome::Captured(region) => {
                    if let Err(e) = capture::capture_and_copy(region, state.save_path.as_deref()) {
                        eprintln!("my-screenshot: {e}");
                        std::process::exit(1);
                    }
                }
            }
            break;
        }
    }

    Ok(())
}

impl AppState {
    fn redraw(&mut self, qh: &QueueHandle<Self>) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        match overlay.render(&self.selection) {
            Ok(buffer) => {
                let surface = self.layer.wl_surface();
                if let Err(e) = buffer.attach_to(surface) {
                    eprintln!("my-screenshot: no se pudo adjuntar el buffer del overlay: {e:?}");
                    return;
                }
                surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
                surface.frame(qh, FrameCallbackData(surface.clone()));
                surface.commit();
            }
            Err(e) => {
                eprintln!("my-screenshot: fallo renderizando el overlay: {e}");
            }
        }
    }

    fn handle_key(&mut self, qh: &QueueHandle<Self>, raw_keysym: u32, utf8: Option<&str>) {
        if self.finished.is_some() {
            return;
        }

        match raw_keysym {
            keysyms::ESCAPE => {
                self.finished = Some(Outcome::Cancelled);
                return;
            }
            keysyms::RETURN | keysyms::KP_ENTER | keysyms::SPACE => {
                if self.selection.confirm() {
                    if self.selection.is_finished() {
                        if let Some(result) = self.selection.result() {
                            // Selección degenerada (ancho o alto 0): puede
                            // ocurrir si el usuario confirma ambas fases
                            // sin mover el punto B, o deshace hasta un
                            // rectángulo de tamaño 0 y confirma. `grim -g`
                            // con geometría "0x..." o "...x0" falla (o, en
                            // algunas versiones, produce un PNG inútil),
                            // así que se trata igual que una cancelación,
                            // con un aviso claro en vez de un
                            // `NonZeroExit` opaco desde `capture.rs`.
                            if result.is_empty() {
                                eprintln!(
                                    "my-screenshot: selección vacía, cancelado"
                                );
                                self.finished = Some(Outcome::Cancelled);
                                return;
                            }
                            let (ox, oy) = self.output_logical_position;
                            let region = CaptureRegion::new(
                                ox + result.x,
                                oy + result.y,
                                result.width,
                                result.height,
                            );
                            self.finished = Some(Outcome::Captured(region));
                            return;
                        }
                    }
                    self.redraw(qh);
                }
                return;
            }
            keysyms::BACKSPACE => {
                if self.selection.undo() {
                    self.redraw(qh);
                }
                return;
            }
            _ => {}
        }

        if let Some(key) = utf8.and_then(|s| s.chars().next()) {
            if let Some(cell) = cell_from_key(key) {
                if self.selection.subdivide(cell) {
                    self.redraw(qh);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// Trait impls requeridos por smithay-client-toolkit
// ---------------------------------------------------------------------

impl CompositorHandler for AppState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
        // v1: no soporta cambios de escala en caliente (el overlay dura
        // segundos); se ignora deliberadamente.
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.redraw(qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: wl_output::WlOutput) {}
    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: wl_output::WlOutput) {}
}

impl ShmHandler for AppState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let keyboard = self
                .seat_state
                .get_keyboard(qh, &seat, None)
                .expect("no se pudo crear wl_keyboard para el seat");
            self.keyboard = Some(keyboard);
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}
}

impl KeyboardHandler for AppState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.handle_key(qh, event.keysym.raw(), event.utf8.as_deref());
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
        // No se necesita acción al soltar: cada tecla relevante actúa al
        // presionar.
    }

    /// Auto-repetición de una tecla mantenida presionada (gestionada por
    /// SCTK según `RepeatInfo` del compositor). La reenviamos a la misma
    /// lógica que una pulsación normal: mantener una tecla de la
    /// cuadrícula mantiene subdividiendo, lo cual es un comportamiento razonable y nadie
    /// mantendría Enter/Escape el tiempo suficiente para que importe.
    fn repeat_key(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.handle_key(qh, event.keysym.raw(), event.utf8.as_deref());
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
    }
}

impl LayerShellHandler for AppState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        // El compositor cerró la capa por su cuenta (p. ej. el monitor
        // desapareció): tratarlo como cancelación, no como un pánico.
        if self.finished.is_none() {
            self.finished = Some(Outcome::Cancelled);
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (mut width, mut height) = configure.new_size;
        if width == 0 || height == 0 {
            // El compositor delega el tamaño en el cliente cuando ancla
            // los 4 bordes; en ese caso usamos el tamaño lógico del
            // primer output conocido como respaldo (el output enfocado
            // real ya se resuelve en `run` antes de entrar al loop, pero
            // el primer configure puede llegar antes de esa resolución).
            if let Some((w, h)) = self
                .output_state
                .outputs()
                .filter_map(|o| self.output_state.info(&o))
                .find_map(|info| info.logical_size)
            {
                width = w.max(1) as u32;
                height = h.max(1) as u32;
            }
        }
        let monitor_rect = Rect::new(0, 0, width as i32, height as i32);

        if !self.configured {
            self.overlay = Some(
                Overlay::new(&self.shm, monitor_rect.width, monitor_rect.height)
                    .expect("no se pudo crear el overlay SHM"),
            );
            self.selection = SelectionState::new(monitor_rect);
            self.configured = true;
        }

        self.redraw(qh);
    }
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
delegate_registry!(AppState);

// SCTK 0.21 unificó delegate_compositor!/delegate_output!/delegate_shm!/
// delegate_seat!/delegate_keyboard!/delegate_layer! en este único macro:
// implementa `Dispatch<I, UserData>` para cualquier `I`/`UserData` interno
// de SCTK cuyo `UserData` implemente `Dispatch2<I, AppState>` — que es
// justamente lo que hacen `SurfaceData`, `OutputData`, `SeatData`,
// `KeyboardData<AppState, _>` y `LayerSurfaceData` puertas adentro,
// delegando en los traits `*Handler` que sí implementamos arriba
// (`CompositorHandler`, `OutputHandler`, `ShmHandler`, `SeatHandler`,
// `KeyboardHandler`, `LayerShellHandler`).
delegate_dispatch2!(AppState);

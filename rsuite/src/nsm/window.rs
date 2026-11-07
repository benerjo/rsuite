//! Position and size of the window of a program managed by a session manager.
//!
//! The geometry is saved with the state of the program and given back to the window
//! when the program restarts. As the screens may have changed in between (other
//! resolution, monitor unplugged...), the window is moved and shrunk when needed so
//! that it can still be seen and grabbed.
//!
//! eframe does not tell where the monitors are, so under a session manager the
//! user interface runs on an event loop owned by this module, which can list them.

use std::sync::{Mutex, PoisonError};

use eframe::egui::{Pos2, Rect, Vec2, ViewportCommand};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

use super::{Values, UI_CONTEXT};

const X: &str = "window.x";
const Y: &str = "window.y";
const WIDTH: &str = "window.width";
const HEIGHT: &str = "window.height";

///The height of the top of the window, used to move it, that must stay visible
const TITLE_HEIGHT: f32 = 32.0;
///The width of the top of the window that must be visible
const MIN_VISIBLE_WIDTH: f32 = 64.0;

///The geometry restored from the session, applied when the window is created
static SAVED: Mutex<Option<Geometry>> = Mutex::new(None);

///Where the window is, in points
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    ///The position of the outer corner of the window. Unknown on some platforms (Wayland)
    pub position: Option<Pos2>,
    ///The size of the content of the window
    pub size: Vec2,
}

impl Geometry {
    fn save(&self, values: &mut Values) {
        if let Some(position) = self.position {
            values.set(X, position.x);
            values.set(Y, position.y);
        }
        values.set(WIDTH, self.size.x);
        values.set(HEIGHT, self.size.y);
    }

    fn load(values: &Values) -> Option<Geometry> {
        let size = Vec2::new(values.get(WIDTH)?, values.get(HEIGHT)?);
        if !(size.x > 0.0 && size.y > 0.0) {
            return None;
        }
        let position = match (values.get(X), values.get(Y)) {
            (Some(x), Some(y)) => Some(Pos2::new(x, y)),
            _ => None,
        };
        Some(Geometry { position, size })
    }

    ///Move and shrink the window so that it is visible on the given monitors.
    /// None is returned if the window is already visible
    fn make_visible(&self, monitors: &[Rect]) -> Option<Geometry> {
        let Some(position) = self.position else {
            //the window will be placed by the system: it only has to fit on a screen
            let largest = monitors.iter().map(|m| m.size()).reduce(|a, b| a.max(b))?;
            let size = self.size.min(largest);
            return (size != self.size).then_some(Geometry {
                position: None,
                size,
            });
        };
        let window = Rect::from_min_size(position, self.size);
        //the first monitor is the main one, used if the window is on no monitor at all
        let mut monitor = *monitors.first()?;
        let mut overlap = 0.0;
        for m in monitors {
            let area = window.intersect(*m);
            let area = if area.is_positive() { area.area() } else { 0.0 };
            if area > overlap {
                overlap = area;
                monitor = *m;
            }
        }

        let title = Rect::from_min_size(position, Vec2::new(self.size.x, TITLE_HEIGHT));
        let title_visible = monitors.iter().any(|m| {
            let visible = title.intersect(*m);
            visible.height() > 0.0 && visible.width() >= MIN_VISIBLE_WIDTH.min(self.size.x)
        });
        let fits = self.size.x <= monitor.width() && self.size.y <= monitor.height();
        if title_visible && fits {
            return None;
        }

        let size = self.size.min(monitor.size());
        let max = (monitor.max - size).max(monitor.min);
        Some(Geometry {
            position: Some(position.clamp(monitor.min, max)),
            size,
        })
    }
}

///Remember the geometry saved in the session, to use it for the window
pub(super) fn restore(values: &Values) {
    *SAVED.lock().unwrap_or_else(PoisonError::into_inner) = Geometry::load(values);
}

///Store the current geometry of the window, if there is one
pub(super) fn save(values: &mut Values) {
    let ctx = UI_CONTEXT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let Some(ctx) = ctx else {
        return;
    };
    let geometry = ctx.input(|i| {
        let viewport = i.viewport();
        Geometry {
            position: viewport.outer_rect.map(|r| r.min),
            size: viewport
                .inner_rect
                .map(|r| r.size())
                .unwrap_or(i.content_rect().size()),
        }
    });
    geometry.save(values);
}

///Start the user interface, as 'eframe::run_native' does. If the program is managed
/// by a session manager, the window gets the position and size saved in the session
pub fn run_window(
    title: &str,
    mut options: eframe::NativeOptions,
    app_creator: eframe::AppCreator<'_>,
) -> Result<(), eframe::Error> {
    if !super::is_session_managed() {
        return eframe::run_native(title, options, app_creator);
    }
    let saved = *SAVED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(saved) = saved {
        options.viewport = options.viewport.with_inner_size(saved.size);
        if let Some(position) = saved.position {
            options.viewport = options.viewport.with_position(position);
        }
    }
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event().build()?;
    let app = eframe::create_native(title, options, app_creator, &event_loop);
    let mut placement = Placement { app, saved };
    event_loop.run_app(&mut placement)?;
    Ok(())
}

///Run the eframe application, moving its window on the screens once it is created
struct Placement<'a> {
    app: eframe::EframeWinitApplication<'a>,
    ///The geometry still to be checked
    saved: Option<Geometry>,
}

impl Placement<'_> {
    ///The monitors, in points. The main one comes first
    fn monitors(event_loop: &ActiveEventLoop) -> Vec<Rect> {
        let primary = event_loop.primary_monitor();
        let mut monitors: Vec<_> = event_loop.available_monitors().collect();
        monitors.sort_by_key(|m| Some(m) != primary.as_ref());
        monitors
            .iter()
            .map(|m| {
                let scale = m.scale_factor() as f32;
                let position = m.position();
                let size = m.size();
                Rect::from_min_size(
                    Pos2::new(position.x as f32, position.y as f32) / scale,
                    Vec2::new(size.width as f32, size.height as f32) / scale,
                )
            })
            .collect()
    }
}

impl ApplicationHandler<eframe::UserEvent> for Placement<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        //the window and the user interface are created here
        self.app.resumed(event_loop);
        let Some(saved) = self.saved.take() else {
            return;
        };
        let Some(fixed) = saved.make_visible(&Self::monitors(event_loop)) else {
            return;
        };
        let ctx = UI_CONTEXT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(ctx) = ctx {
            if let Some(position) = fixed.position {
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
            }
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(fixed.size));
            ctx.request_repaint();
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.app.new_events(event_loop, cause);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: eframe::UserEvent) {
        self.app.user_event(event_loop, event);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(event_loop, id, event);
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.app.device_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.app.about_to_wait(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.app.suspended(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.app.exiting(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.app.memory_warning(event_loop);
    }
}

#[cfg(test)]
mod test {
    use eframe::egui::{Pos2, Rect, Vec2};

    use super::{Geometry, Values};

    fn geometry(x: f32, y: f32, width: f32, height: f32) -> Geometry {
        Geometry {
            position: Some(Pos2::new(x, y)),
            size: Vec2::new(width, height),
        }
    }

    fn monitor(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height))
    }

    #[test]
    fn save_and_load() {
        let saved = geometry(-10.5, 20.0, 320.0, 640.0);
        let mut values = Values::new();
        saved.save(&mut values);
        assert_eq!(Geometry::load(&values), Some(saved));

        let mut values = Values::new();
        let no_position = Geometry {
            position: None,
            size: Vec2::new(300.0, 200.0),
        };
        no_position.save(&mut values);
        assert_eq!(Geometry::load(&values), Some(no_position));

        assert_eq!(Geometry::load(&Values::new()), None);
        values.set(super::WIDTH, 0.0);
        assert_eq!(Geometry::load(&values), None);
    }

    #[test]
    fn visible_windows_are_not_moved() {
        let monitors = [
            monitor(0.0, 0.0, 1920.0, 1080.0),
            monitor(1920.0, 0.0, 1280.0, 1024.0),
        ];
        assert_eq!(
            geometry(100.0, 100.0, 320.0, 640.0).make_visible(&monitors),
            None
        );
        //on the second monitor
        assert_eq!(
            geometry(2000.0, 300.0, 320.0, 640.0).make_visible(&monitors),
            None
        );
        //across both monitors
        assert_eq!(
            geometry(1800.0, 100.0, 320.0, 640.0).make_visible(&monitors),
            None
        );
        //partly outside, but the top can still be grabbed
        assert_eq!(
            geometry(-200.0, 900.0, 320.0, 640.0).make_visible(&monitors),
            None
        );
    }

    #[test]
    fn hidden_windows_are_moved() {
        let monitors = [
            monitor(0.0, 0.0, 1920.0, 1080.0),
            monitor(1920.0, 0.0, 1280.0, 1024.0),
        ];
        //the second monitor was unplugged
        assert_eq!(
            geometry(2000.0, 300.0, 320.0, 640.0).make_visible(&monitors[..1]),
            Some(geometry(1600.0, 300.0, 320.0, 640.0))
        );
        //title above the screen
        assert_eq!(
            geometry(100.0, -100.0, 320.0, 640.0).make_visible(&monitors),
            Some(geometry(100.0, 0.0, 320.0, 640.0))
        );
        //far away: moved on the main monitor
        assert_eq!(
            geometry(-5000.0, 5000.0, 320.0, 640.0).make_visible(&monitors),
            Some(geometry(0.0, 440.0, 320.0, 640.0))
        );
        //bigger than the screen after a change of resolution
        assert_eq!(
            geometry(0.0, 0.0, 1024.0, 1000.0).make_visible(&[monitor(0.0, 0.0, 800.0, 600.0)]),
            Some(geometry(0.0, 0.0, 800.0, 600.0))
        );
        //no position known: only the size is reduced
        let no_position = Geometry {
            position: None,
            size: Vec2::new(1024.0, 500.0),
        };
        assert_eq!(no_position.make_visible(&monitors), None);
        assert_eq!(
            no_position.make_visible(&[monitor(0.0, 0.0, 800.0, 600.0)]),
            Some(Geometry {
                position: None,
                size: Vec2::new(800.0, 500.0)
            })
        );
        //no monitor known: nothing can be done
        assert_eq!(geometry(-5000.0, 0.0, 320.0, 640.0).make_visible(&[]), None);
    }
}

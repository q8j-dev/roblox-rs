use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

pub use winit;

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Key {
        code: u32,
        pressed: bool,
        text: Option<String>,
    },
    MouseButton {
        button: u8,
        pressed: bool,
        x: f32,
        y: f32,
    },
    MouseMove {
        x: f32,
        y: f32,
    },
    MouseDelta {
        dx: f32,
        dy: f32,
    },
    Wheel {
        delta: f32,
    },
    Resized {
        width: u32,
        height: u32,
    },
    Focus(bool),
}

pub trait App {
    fn init(&mut self, window: Arc<Window>);
    fn input(&mut self, input: Input);
    fn frame(&mut self, dt: f64);
    fn should_exit(&self) -> bool {
        false
    }
}

pub fn roblox_keycode(code: KeyCode) -> u32 {
    use KeyCode::*;
    match code {
        Backspace => 8,
        Tab => 9,
        Enter => 13,
        Pause => 19,
        Escape => 27,
        Space => 32,
        Quote => 39,
        Comma => 44,
        Minus => 45,
        Period => 46,
        Slash => 47,
        Digit0 => 48,
        Digit1 => 49,
        Digit2 => 50,
        Digit3 => 51,
        Digit4 => 52,
        Digit5 => 53,
        Digit6 => 54,
        Digit7 => 55,
        Digit8 => 56,
        Digit9 => 57,
        Semicolon => 59,
        Equal => 61,
        BracketLeft => 91,
        Backslash => 92,
        BracketRight => 93,
        Backquote => 96,
        KeyA => 97,
        KeyB => 98,
        KeyC => 99,
        KeyD => 100,
        KeyE => 101,
        KeyF => 102,
        KeyG => 103,
        KeyH => 104,
        KeyI => 105,
        KeyJ => 106,
        KeyK => 107,
        KeyL => 108,
        KeyM => 109,
        KeyN => 110,
        KeyO => 111,
        KeyP => 112,
        KeyQ => 113,
        KeyR => 114,
        KeyS => 115,
        KeyT => 116,
        KeyU => 117,
        KeyV => 118,
        KeyW => 119,
        KeyX => 120,
        KeyY => 121,
        KeyZ => 122,
        Delete => 127,
        Numpad0 => 256,
        Numpad1 => 257,
        Numpad2 => 258,
        Numpad3 => 259,
        Numpad4 => 260,
        Numpad5 => 261,
        Numpad6 => 262,
        Numpad7 => 263,
        Numpad8 => 264,
        Numpad9 => 265,
        NumpadDecimal => 266,
        NumpadDivide => 267,
        NumpadMultiply => 268,
        NumpadSubtract => 269,
        NumpadAdd => 270,
        NumpadEnter => 271,
        NumpadEqual => 272,
        ArrowUp => 273,
        ArrowDown => 274,
        ArrowRight => 275,
        ArrowLeft => 276,
        Insert => 277,
        Home => 278,
        End => 279,
        PageUp => 280,
        PageDown => 281,
        F1 => 282,
        F2 => 283,
        F3 => 284,
        F4 => 285,
        F5 => 286,
        F6 => 287,
        F7 => 288,
        F8 => 289,
        F9 => 290,
        F10 => 291,
        F11 => 292,
        F12 => 293,
        NumLock => 300,
        CapsLock => 301,
        ScrollLock => 302,
        ShiftRight => 303,
        ShiftLeft => 304,
        ControlRight => 305,
        ControlLeft => 306,
        AltRight => 307,
        AltLeft => 308,
        SuperLeft => 311,
        SuperRight => 312,
        _ => 0,
    }
}

struct Runner<A: App> {
    app: A,
    title: String,
    size: (u32, u32),
    window: Option<Arc<Window>>,
    last: Instant,
    cursor: (f32, f32),
}

impl<A: App> ApplicationHandler for Runner<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.title.clone())
            .with_inner_size(LogicalSize::new(self.size.0, self.size.1));
        match event_loop.create_window(attrs) {
            Ok(w) => {
                let w = Arc::new(w);
                self.app.init(w.clone());
                self.window = Some(w);
                self.last = Instant::now();
            }
            Err(e) => {
                tracing::error!("could not create a window: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.app.input(Input::Resized {
                width: size.width,
                height: size.height,
            }),
            WindowEvent::Focused(f) => self.app.input(Input::Focus(f)),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.app.input(Input::Key {
                        code: roblox_keycode(code),
                        pressed: event.state == ElementState::Pressed,
                        text: event.text.map(|t| t.to_string()),
                    });
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let PhysicalPosition { x, y } = position;
                self.cursor = (x as f32, y as f32);
                self.app.input(Input::MouseMove {
                    x: x as f32,
                    y: y as f32,
                });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => 1,
                    MouseButton::Right => 2,
                    MouseButton::Middle => 3,
                    _ => 0,
                };
                self.app.input(Input::MouseButton {
                    button: b,
                    pressed: state == ElementState::Pressed,
                    x: self.cursor.0,
                    y: self.cursor.1,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.app.input(Input::Wheel { delta: d });
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last).as_secs_f64();
                self.last = now;
                self.app.frame(dt);
                if self.app.should_exit() {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.app.input(Input::MouseDelta {
                dx: delta.0 as f32,
                dy: delta.1 as f32,
            });
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

pub fn run(title: &str, size: (u32, u32), app: impl App) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut runner = Runner {
        app,
        title: title.to_string(),
        size,
        window: None,
        last: Instant::now(),
        cursor: (0.0, 0.0),
    };
    event_loop.run_app(&mut runner).map_err(|e| e.to_string())
}

pub fn user_data_dir(app: &str) -> std::path::PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support").join(app)
    } else if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or(home)
            .join(app)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"))
            .join(app)
    }
}

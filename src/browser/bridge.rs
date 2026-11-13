use std::ffi::{CStr, CString};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::{env, io, thread};

use libc::{c_char, c_float, c_int, c_uchar, c_uint, c_void, size_t};

use crate::cli::{CommandLine, CommandLineProgram, EnvVar};
use crate::gfx::{Cast, Color, Point, Rect, Size};
use crate::output::{RenderThread, Window};
use crate::ui::navigation::NavigationAction;
use crate::{input, utils::log};

#[repr(C)]
#[derive(Copy, Clone)]
pub struct CSize {
    width: c_uint,
    height: c_uint,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CPoint {
    x: c_uint,
    y: c_uint,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CRect {
    origin: CPoint,
    size: CSize,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CColor {
    r: u8,
    g: u8,
    b: u8,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CText {
    text: *const c_char,
    rect: CRect,
    color: CColor,
}

#[repr(C)]
pub struct RendererBridge {
    cmd: CommandLine,
    window: Window,
    renderer: RenderThread,
}

unsafe impl Send for RendererBridge {}
unsafe impl Sync for RendererBridge {}

pub type RendererPtr = *const Mutex<RendererBridge>;

impl<T: Copy> From<CPoint> for Point<T>
where
    c_uint: Cast<T>,
{
    fn from(value: CPoint) -> Self {
        Point::new(value.x, value.y).cast()
    }
}
impl From<Size<c_uint>> for CSize {
    fn from(value: Size<c_uint>) -> Self {
        Self {
            width: value.width,
            height: value.height,
        }
    }
}
impl<T: Copy> From<CSize> for Size<T>
where
    c_uint: Cast<T>,
{
    fn from(value: CSize) -> Self {
        Size::new(value.width, value.height).cast()
    }
}
impl From<CColor> for Color {
    fn from(value: CColor) -> Self {
        Color::new(value.r, value.g, value.b)
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct BrowserDelegate {
    shutdown: extern "C" fn(),
    refresh: extern "C" fn(),
    go_to: extern "C" fn(*const c_char),
    go_back: extern "C" fn(),
    go_forward: extern "C" fn(),
    scroll: extern "C" fn(c_int),
    key_press: extern "C" fn(c_char),
    mouse_up: extern "C" fn(c_uint, c_uint),
    mouse_down: extern "C" fn(c_uint, c_uint),
    mouse_move: extern "C" fn(c_uint, c_uint),
    post_task: extern "C" fn(extern "C" fn(*mut c_void), *mut c_void),
}

fn main() -> io::Result<Option<i32>> {
    let cmd = match CommandLineProgram::parse_or_run() {
        None => return Ok(Some(0)),
        Some(cmd) => cmd,
    };

    if cmd.shell_mode {
        return Ok(None);
    }

    let mut terminal = input::Terminal::setup();
    let mut command = Command::new(env::current_exe()?);

    if !cmd.bitmap {
        command
            .arg("--disable-threaded-scrolling")
            .arg("--disable-threaded-animation");
    }

    // No page named: reopen the saved tabs on their active tab, else open
    // the home page (bookmarks) instead of about:blank.
    let mut args = cmd.args;

    if !crate::home::has_url_arg(&args) {
        use crate::ui::{navigation::ENV_RESTORED, tabs};

        match tabs::Tabs::saved_active_url(tabs::session_path()) {
            Some(url) => {
                command.env(ENV_RESTORED, "1");
                args.push(url)
            }
            None => args.push(crate::home::url()),
        }
    }

    let output = command
        .args(args)
        .env(EnvVar::ShellMode, "1")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .output()?;

    terminal.teardown();

    let code = output.status.code().unwrap_or(127);

    if code != 0 || cmd.debug {
        io::stderr().write_all(&output.stderr)?;
    }

    Ok(Some(code))
}

#[no_mangle]
pub extern "C" fn carbonyl_bridge_main() {
    if let Some(code) = main().unwrap() {
        std::process::exit(code)
    }
}

#[no_mangle]
pub extern "C" fn carbonyl_bridge_bitmap_mode() -> bool {
    CommandLine::parse().bitmap
}

#[no_mangle]
pub extern "C" fn carbonyl_bridge_get_dpi() -> c_float {
    Window::read().dpi
}

#[no_mangle]
pub extern "C" fn carbonyl_renderer_create() -> RendererPtr {
    let bridge = RendererBridge {
        cmd: CommandLine::parse(),
        window: Window::read(),
        renderer: RenderThread::new(),
    };

    Box::into_raw(Box::new(Mutex::new(bridge)))
}

#[no_mangle]
pub extern "C" fn carbonyl_renderer_start(bridge: RendererPtr) {
    {
        let bridge = unsafe { bridge.as_ref() };
        let mut bridge = bridge.unwrap().lock().unwrap();

        bridge.renderer.enable()
    }

    carbonyl_renderer_resize(bridge);
}

#[no_mangle]
pub extern "C" fn carbonyl_renderer_resize(bridge: RendererPtr) {
    let bridge = unsafe { bridge.as_ref() };
    let mut bridge = bridge.unwrap().lock().unwrap();
    let window = bridge.window.update();
    let cells = window.cells.clone();
    let ui_rows = window.ui_rows;

    log::debug!("resizing renderer, terminal window: {:?}", window);

    bridge
        .renderer
        .render(move |renderer| renderer.set_layout(cells, ui_rows));
}

#[no_mangle]
pub extern "C" fn carbonyl_renderer_push_nav(
    bridge: RendererPtr,
    url: *const c_char,
    can_go_back: bool,
    can_go_forward: bool,
) {
    let (bridge, url) = unsafe { (bridge.as_ref(), CStr::from_ptr(url)) };
    let (mut bridge, url) = (bridge.unwrap().lock().unwrap(), url.to_owned());

    bridge.renderer.render(move |renderer| {
        if let Some(script) = renderer.push_nav(url.to_str().unwrap(), can_go_back, can_go_forward) {
            go_to(script)
        }
    });
}

#[no_mangle]
pub extern "C" fn carbonyl_renderer_set_title(bridge: RendererPtr, title: *const c_char) {
    let (bridge, title) = unsafe { (bridge.as_ref(), CStr::from_ptr(title)) };
    let (mut bridge, title) = (bridge.unwrap().lock().unwrap(), title.to_owned());

    bridge
        .renderer
        .render(move |renderer| renderer.set_title(title.to_str().unwrap()).unwrap());
}

#[no_mangle]
pub extern "C" fn carbonyl_renderer_draw_text(
    bridge: RendererPtr,
    text: *const CText,
    text_size: size_t,
) {
    let (bridge, text) = unsafe { (bridge.as_ref(), std::slice::from_raw_parts(text, text_size)) };
    let mut bridge = bridge.unwrap().lock().unwrap();
    let mut vec = text
        .iter()
        .map(|text| {
            let str = unsafe { CStr::from_ptr(text.text) };

            (
                str.to_str().unwrap().to_owned(),
                text.rect.origin.into(),
                text.rect.size.into(),
                text.color.into(),
            )
        })
        .collect::<Vec<(String, Point, Size, Color)>>();

    bridge.renderer.render(move |renderer| {
        renderer.clear_text();

        for (text, origin, size, color) in std::mem::take(&mut vec) {
            renderer.draw_text(&text, origin, size, color)
        }
    });
}

#[derive(Clone, Copy)]
struct CallbackData(*const c_void);

impl CallbackData {
    pub fn as_ptr(&self) -> *const c_void {
        self.0
    }
}

unsafe impl Send for CallbackData {}
unsafe impl Sync for CallbackData {}

#[no_mangle]
pub extern "C" fn carbonyl_renderer_draw_bitmap(
    bridge: RendererPtr,
    pixels: *const c_uchar,
    pixels_size: CSize,
    rect: CRect,
    callback: extern "C" fn(*const c_void),
    callback_data: *const c_void,
) {
    let length = (pixels_size.width * pixels_size.height * 4) as usize;
    let (bridge, pixels) = unsafe { (bridge.as_ref(), std::slice::from_raw_parts(pixels, length)) };
    let callback_data = CallbackData(callback_data);
    let mut bridge = bridge.unwrap().lock().unwrap();

    bridge.renderer.render(move |renderer| {
        renderer.draw_background(
            pixels,
            pixels_size.into(),
            Rect {
                size: rect.size.into(),
                origin: rect.origin.into(),
            },
        );

        callback(callback_data.as_ptr());
    });
}

#[no_mangle]
pub extern "C" fn carbonyl_renderer_get_size(bridge: RendererPtr) -> CSize {
    let bridge = unsafe { bridge.as_ref() };
    let bridge = bridge.unwrap().lock().unwrap();

    log::debug!("terminal size: {:?}", bridge.window.browser);

    bridge.window.browser.into()
}

/// Chromium's input functions, kept once input starts so page loads can be
/// started from any callback (tab switches, scroll restores).
static DELEGATE: OnceLock<BrowserDelegate> = OnceLock::new();

/// Load `url` (or run a `javascript:` URL) on the browser main thread. The
/// string moves into the task: Chromium reads it when the task runs, later.
fn go_to(url: String) {
    let Some(&delegate) = DELEGATE.get() else { return };
    let Ok(url) = CString::new(url) else { return };

    unsafe { post_task(delegate.post_task, move || (delegate.go_to)(url.as_ptr())) }
}

extern "C" fn post_task_handler(callback: *mut c_void) {
    let mut closure = unsafe { Box::from_raw(callback as *mut Box<dyn FnMut()>) };

    closure()
}

/// `post_task` for the other browser modules.
pub(super) unsafe fn post_task_raw<F>(handle: extern "C" fn(extern "C" fn(*mut c_void), *mut c_void), run: F)
where
    F: FnMut() + Send + 'static,
{
    post_task(handle, run)
}

unsafe fn post_task<F>(handle: extern "C" fn(extern "C" fn(*mut c_void), *mut c_void), run: F)
where
    F: FnMut() + Send + 'static,
{
    let closure: *mut Box<dyn FnMut()> = Box::into_raw(Box::new(Box::new(run)));

    handle(post_task_handler, closure as *mut c_void);
}

/// Function called by the C++ code to listen for input events.
///
/// This will block so the calling code should start and own a dedicated thread.
/// It will panic if there is any error.
#[no_mangle]
pub extern "C" fn carbonyl_renderer_listen(bridge: RendererPtr, delegate: *mut BrowserDelegate) {
    let bridge = unsafe { &*bridge };
    let delegate = unsafe { *delegate };
    let _ = DELEGATE.set(delegate);

    // Chromium has installed its SIGWINCH handler by now; run it on the
    // browser main thread instead of whichever thread gets the signal.
    if !super::resize_signal::route_to_main_thread(delegate.post_task) {
        log::warning!("could not route SIGWINCH to the browser main thread");
    }

    use input::*;

    thread::spawn(move || {
        macro_rules! emit {
            ($event:ident($($args:expr),*) => $closure:expr) => {{
                let run = move || {
                    (delegate.$event)($($args),*);

                    $closure
                };

                unsafe { post_task(delegate.post_task, run) }
            }};
            ($event:ident($($args:expr),*)) => {{
                emit!($event($($args),*) => {})
            }};
        }

        listen(|mut events| {
            bridge.lock().unwrap().renderer.render(move |renderer| {
                let get_scale = || bridge.lock().unwrap().window.scale;
                // Page pixels of a terminal cell, below the UI rows.
                let ui_rows = renderer.ui_rows() as f32;
                // A cell showing page text maps to where Chromium painted that
                // text (`Renderer::click_y`: page device pixels, 4 to a cell row, the
                // UI rows not included).
                let scale = |col, row, text_y: Option<i32>| {
                    let scale = get_scale();
                    let y = match text_y {
                        Some(y) => y as f32 / 4.0,
                        None => row as f32 - ui_rows + 0.5,
                    };

                    scale
                        .mul(((col as f32 + 0.5), y))
                        .floor()
                        .cast()
                        .into()
                };
                let dispatch = |action, text_y: Option<i32>| {
                    match action {
                        NavigationAction::Ignore => (),
                        NavigationAction::Forward => return true,
                        NavigationAction::GoBack() => emit!(go_back()),
                        NavigationAction::GoForward() => emit!(go_forward()),
                        NavigationAction::Refresh() => emit!(refresh()),
                        NavigationAction::GoTo(url) => go_to(url),
                        NavigationAction::Run(urls) => urls.into_iter().for_each(go_to),
                        NavigationAction::OpenLinkAt(cell) => {
                            let (x, y): (u32, u32) = scale(cell.x as usize, cell.y as usize, text_y);

                            log::debug!("open link at cell {cell:?} = px {x},{y}");
                            go_to(crate::ui::navigation::open_link_script(x as i32, y as i32))
                        }
                        NavigationAction::ProbeAt(cell) => {
                            let (x, y): (u32, u32) = scale(cell.x as usize, cell.y as usize, text_y);

                            log::debug!("probe at cell {cell:?} = px {x},{y}");
                            go_to(crate::ui::navigation::probe_script(x as i32, y as i32))
                        }
                        // Done by the renderer; never reaches here.
                        NavigationAction::Copy(_) => (),
                        NavigationAction::Save { url, referer, image } => {
                            thread::spawn(move || {
                                let dir = crate::download::dir();
                                let status = match crate::download::save(&url, referer.as_deref(), image, &dir) {
                                    Ok(path) => format!("saved to {}", path.display()),
                                    Err(message) => message,
                                };

                                log::debug!("save {url}: {status}");
                                bridge.lock().unwrap().renderer.render(move |r| r.set_status(Some(status.clone())));
                            });
                        }
                    };

                    return false;
                };

                for event in std::mem::take(&mut events) {
                    use Event::*;

                    match event {
                        Exit => (),
                        Scroll { delta } => {
                            let scale = get_scale();

                            emit!(scroll((delta as f32 * scale.height) as c_int))
                        }
                        ScrollX { delta } => {
                            // Chromium's scroll call is vertical only; scroll
                            // sideways from inside the page, 6 columns a notch.
                            let px = (delta as f32 * 6.0 * get_scale().width).round();

                            go_to(format!("javascript:void(window.scrollBy({px},0))"))
                        }
                        KeyPress { key } => {
                            if dispatch(renderer.keypress(&key).unwrap(), None) {
                                emit!(key_press(key.char as c_char))
                            }
                        }
                        MouseUp { col, row, button } => {
                            let text_y = renderer.click_y(col, row);
                            if dispatch(renderer.mouse_up((col as _, row as _).into(), button).unwrap(), text_y) {
                                let (width, height) = scale(col, row, text_y);

                                emit!(mouse_up(width, height))
                            }
                        }
                        MouseDown { col, row, button } => {
                            let text_y = renderer.click_y(col, row);
                            if dispatch(renderer.mouse_down((col as _, row as _).into(), button).unwrap(), text_y) {
                                let (width, height) = scale(col, row, text_y);

                                emit!(mouse_down(width, height))
                            }
                        }
                        MouseMove { col, row } => {
                            let text_y = renderer.click_y(col, row);
                            if dispatch(renderer.mouse_move((col as _, row as _).into()).unwrap(), text_y) {
                                let (width, height) = scale(col, row, text_y);

                                emit!(mouse_move(width, height))
                            }
                        }
                        Terminal(terminal) => match terminal {
                            TerminalEvent::Name(name) => log::debug!("terminal name: {name}"),
                            TerminalEvent::TrueColorSupported => renderer.enable_true_color(),
                        },
                    }
                }
            })
        })
        .unwrap();

        // Setup single-use channel
        let (tx, rx) = mpsc::channel();

        // Signal the browser to shutdown and notify our thread
        emit!(shutdown() => tx.send(()).unwrap());
        rx.recv().unwrap();

        // Shutdown rendering thread
        // if let Some(handle) = { bridge.lock().unwrap().renderer().stop() } {
        //     handle.join().unwrap()
        // }
    });
}

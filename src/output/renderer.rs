use std::{
    io::{self, Write},
    rc::Rc,
};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    gfx::{Color, Point, Rect, Size},
    input::Key,
    ui::navigation::{Navigation, NavigationAction},
    utils::log,
};

use super::{Cell, Grapheme, Painter};

pub struct Renderer {
    nav: Navigation,
    cells: Vec<(Cell, Cell)>,
    painter: Painter,
    /// The page in cells; the buffer also has `ui_rows` rows above it.
    size: Size,
    ui_rows: u32,
    /// Per cell of `cells`: the device pixel row of the baseline of the page
    /// text drawn there. Text is squeezed into one terminal row, so its row
    /// drifts from Chromium's own layout (more so when zoomed); a click on
    /// drawn text goes to where Chromium painted it (see `click_y`).
    text_y: Vec<Option<i32>>,
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer {
            nav: Navigation::new(),
            cells: Vec::with_capacity(0),
            painter: Painter::new(),
            size: Size::new(0, 0),
            ui_rows: 1,
            text_y: Vec::new(),
        }
    }

    /// UI rows above the page in the cell buffer now.
    pub fn ui_rows(&self) -> u32 {
        self.ui_rows
    }

    pub fn enable_true_color(&mut self) {
        self.painter.set_true_color(true)
    }

    pub fn keypress(&mut self, key: &Key) -> io::Result<NavigationAction> {
        let action = self.nav.keypress(key);

        self.sync_ui_rows();
        self.copy(action)
    }

    /// Does a `Copy` action here: an OSC 52 clipboard store, which tetron-wm
    /// and most terminals take. Other actions go back to the caller.
    fn copy(&mut self, action: NavigationAction) -> io::Result<NavigationAction> {
        let NavigationAction::Copy(text) = action else { return Ok(action) };
        let mut stdout = io::stdout();

        write!(stdout, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
        stdout.flush()?;
        log::debug!("copied {text}");
        self.nav.set_status(Some(format!("copied {text}")));

        Ok(NavigationAction::Ignore)
    }

    /// Background tabs whose title is to be fetched, as (id, url).
    pub fn take_title_fetches(&mut self) -> Vec<(u64, String)> {
        self.nav.take_title_fetches()
    }

    pub fn set_fetched_title(&mut self, id: u64, url: &str, title: &str) {
        self.nav.set_fetched_title(id, url, title)
    }

    /// A short message in place of the URL (`None` clears it).
    pub fn set_status(&mut self, status: Option<String>) {
        self.nav.set_status(status)
    }

    /// Ask for a resize when the tab strip appears or goes: the window then
    /// gives the page one row less or more (see `ui::ui_rows`).
    fn sync_ui_rows(&mut self) {
        if crate::ui::set_ui_rows(self.nav.ui_rows()) {
            unsafe { libc::raise(libc::SIGWINCH) };
        }
    }

    pub fn mouse_up(&mut self, origin: Point, button: u8) -> io::Result<NavigationAction> {
        let action = self.nav.mouse_up(origin, button);

        Ok(action)
    }
    pub fn mouse_down(&mut self, origin: Point, button: u8) -> io::Result<NavigationAction> {
        let action = self.nav.mouse_down(origin, button);

        self.sync_ui_rows();
        self.copy(action)
    }
    pub fn mouse_move(&mut self, origin: Point) -> io::Result<NavigationAction> {
        let action = self.nav.mouse_move(origin);

        Ok(action)
    }

    /// Returns a script to run in the new page (scroll restore on tab switch).
    pub fn push_nav(&mut self, url: &str, can_go_back: bool, can_go_forward: bool) -> Option<String> {
        let script = self.nav.push(url, can_go_back, can_go_forward);

        self.sync_ui_rows();
        script
    }

    pub fn get_size(&self) -> Size {
        self.size
    }

    pub fn set_size(&mut self, size: Size) {
        self.set_layout(size, 1)
    }

    /// The page size and the UI rows above it, from the window on resize.
    pub fn set_layout(&mut self, size: Size, ui_rows: u32) {
        self.nav.set_size(size, ui_rows);
        self.size = size;
        self.ui_rows = ui_rows;

        let mut x = 0;
        let mut y = 0;
        let bound = size.width - 1;
        let cells = (size.width * ui_rows + size.width * size.height) as usize;

        self.text_y.clear();
        self.text_y.resize(cells, None);
        self.cells.clear();
        self.cells.resize_with(cells, || {
            let cell = (Cell::new(x, y), Cell::new(x, y));

            if x < bound {
                x += 1;
            } else {
                x = 0;
                y += 1;
            }

            cell
        });
    }

    pub fn render(&mut self) -> io::Result<()> {
        let size = self.size;

        for (origin, element) in self.nav.render(size) {
            if origin.y >= self.ui_rows as i32 {
                continue;
            }

            self.fill_rect(
                Rect::new(origin.x, origin.y, element.text.width() as u32, 1),
                element.background,
            );
            self.draw_ui_text(&element.text, origin, element.foreground);
        }

        // The menu is painted over the page without touching the page cells,
        // so the page shows again where it was when the menu goes.
        let mut overlay = std::collections::HashMap::new();
        let width = self.size.width as i32;

        for (origin, element) in self.nav.render_overlay() {
            for (i, grapheme) in UnicodeSegmentation::graphemes(element.text.as_str(), true).enumerate() {
                let x = origin.x + i as i32;
                let at = origin.y * width + x;

                if x < 0 || x >= width || origin.y < 0 || at as usize >= self.cells.len() {
                    continue;
                }

                let mut cell = Cell::new(x as u32, origin.y as u32);
                let background = element.background;

                cell.quadrant = (background, background, background, background);
                cell.grapheme = Some(Rc::new(Grapheme {
                    color: element.foreground,
                    index: 0,
                    width: 1,
                    char: grapheme.to_owned(),
                }));
                overlay.insert(at as usize, cell);
            }
        }

        self.painter.begin()?;

        for (index, (previous, current)) in self.cells.iter_mut().enumerate() {
            let shown = overlay.get(&index).unwrap_or(current);

            if shown == previous {
                continue;
            }

            previous.quadrant = shown.quadrant;
            previous.grapheme = shown.grapheme.clone();

            self.painter.paint(shown)?;
        }

        self.painter.end(self.nav.cursor())?;

        Ok(())
    }

    /// Draw the background from a pixel array encoded in RGBA8888
    pub fn draw_background(&mut self, pixels: &[u8], pixels_size: Size, rect: Rect) {
        let viewport = self.size.cast::<usize>();

        if pixels.len() < viewport.width * viewport.height * 8 * 4 {
            log::debug!(
                "unexpected size, actual: {}, expected: {}",
                pixels.len(),
                viewport.width * viewport.height * 8 * 4
            );
            return;
        }

        let origin = rect.origin.cast::<f32>().max(0.0) / (2.0, 4.0);
        let size = rect.size.cast::<f32>().max(0.0) / (2.0, 4.0);
        let top = (origin.y.floor() as usize).min(viewport.height);
        let left = (origin.x.floor() as usize).min(viewport.width);
        let right = ((origin.x + size.width).ceil() as usize)
            .min(viewport.width)
            .max(left);
        let bottom = ((origin.y + size.height).ceil() as usize)
            .min(viewport.height)
            .max(top);
        let row_length = pixels_size.width as usize;
        let pixel = |x, y| {
            Color::new(
                pixels[((x + y * row_length) * 4 + 2) as usize],
                pixels[((x + y * row_length) * 4 + 1) as usize],
                pixels[((x + y * row_length) * 4 + 0) as usize],
            )
        };
        let pair = |x, y| pixel(x, y).avg_with(pixel(x, y + 1));

        let ui_rows = self.ui_rows as usize;

        for y in top..bottom {
            let index = (y + ui_rows) * viewport.width;
            let start = index + left;
            let end = index + right;
            let (mut x, y) = (left * 2, y * 4);

            for (_, cell) in &mut self.cells[start..end] {
                cell.quadrant = (
                    pair(x + 0, y + 0),
                    pair(x + 1, y + 0),
                    pair(x + 1, y + 2),
                    pair(x + 0, y + 2),
                );

                x += 2;
            }
        }
    }

    pub fn clear_text(&mut self) {
        for (_, cell) in self.cells.iter_mut() {
            cell.grapheme = None
        }
        self.text_y.fill(None);
    }

    /// The device pixel row a click on terminal cell (`col`, `row`) should
    /// reach when page text is drawn there: just above that text's baseline,
    /// inside its glyph box, so a click on a link's text hits the link at any
    /// zoom. `None` for a cell without page text (map it geometrically).
    pub fn click_y(&self, col: usize, row: usize) -> Option<i32> {
        let index = row * self.size.width as usize + col;
        self.text_y.get(index).copied().flatten().map(|y| y - 1)
    }

    pub fn set_title(&mut self, title: &str) -> io::Result<()> {
        let shown = self.nav.set_title(title);

        self.sync_ui_rows();

        if !shown {
            return Ok(());
        }

        let mut stdout = io::stdout();

        write!(stdout, "\x1b]0;{title}\x07")?;
        write!(stdout, "\x1b]1;{title}\x07")?;
        write!(stdout, "\x1b]2;{title}\x07")?;

        stdout.flush()
    }

    /// Text in the UI rows at a cell position (no page offset, no wrapping).
    fn draw_ui_text(&mut self, text: &str, origin: Point, color: Color) {
        let width = self.size.width as i32;
        let mut x = origin.x;

        for grapheme in UnicodeSegmentation::graphemes(text, true) {
            let w = grapheme.width();

            for index in 0..w {
                if x < 0 || x >= width {
                    return;
                }

                let at = (origin.y * width + x) as usize;

                if let Some((_, cell)) = self.cells.get_mut(at) {
                    let next = Grapheme { color, index, width: w, char: grapheme.to_string() };

                    if cell.grapheme.as_ref().map_or(true, |g| g.color != next.color || g.char != next.char) {
                        cell.grapheme = Some(Rc::new(next));
                    }
                }

                x += 1;
            }
        }
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.draw(rect, |cell| {
            cell.grapheme = None;
            cell.quadrant = (color, color, color, color);
        })
    }

    pub fn draw<F>(&mut self, bounds: Rect, mut draw: F)
    where
        F: FnMut(&mut Cell),
    {
        // Clip the rectangle to the cell buffer: in a very small terminal
        // Chromium still asks for rectangles (such as the navigation bar)
        // wider than the viewport, which indexed past the buffer and killed
        // the browser on resize.
        let width = self.size.width as i64;

        if width <= 0 {
            return;
        }

        let rows = self.cells.len() as i64 / width;
        let (x, y) = (bounds.origin.x as i64, bounds.origin.y as i64);
        let (w, h) = (bounds.size.width as i64, bounds.size.height as i64);
        let left = x.clamp(0, width);
        let right = (x + w).clamp(left, width);
        let top = y.clamp(0, rows);
        let bottom = (y + h).clamp(top, rows);

        // Iterate over each row
        for row in top..bottom {
            let start = (row * width + left) as usize;
            let end = (row * width + right) as usize;

            for (_, current) in self.cells[start..end].iter_mut() {
                draw(current)
            }
        }
    }

    /// Render some text into the terminal output
    pub fn draw_text(&mut self, string: &str, origin: Point, size: Size, color: Color) {
        // Get an iterator starting at the text origin
        let len = self.cells.len();
        let viewport = &self.size.cast::<usize>();

        if size.width > 2 && size.height > 2 {
            let origin = (origin.cast::<f32>() / (2.0, 4.0) + (0.0, self.ui_rows as f32)).round();
            let size = (size.cast::<f32>() / (2.0, 4.0)).round();
            let left = (origin.x.max(0.0) as usize).min(viewport.width);
            let right = ((origin.x + size.width).max(0.0) as usize).min(viewport.width);
            let top = (origin.y.max(0.0) as usize).min(viewport.height);
            let bottom = ((origin.y + size.height).max(0.0) as usize).min(viewport.height);

            for y in top..bottom {
                let index = y * viewport.width;
                let start = index + left;
                let end = index + right;

                for (_, cell) in self.cells[start..end].iter_mut() {
                    cell.grapheme = None
                }
            }
        } else {
            // Compute the buffer index based on the position
            // Page text sits below the UI rows, like the page background (it
            // used to sit one row too high, above the place clicks reach).
            let index = origin.x / 2 + ((origin.y + 1) / 4 + self.ui_rows as i32) * (viewport.width as i32);
            let start = len.min(index.max(0) as usize);
            let mut at = start;
            let mut iter = self.cells[start..].iter_mut();

            // Get every Unicode grapheme in the input string
            for grapheme in UnicodeSegmentation::graphemes(string, true) {
                let width = grapheme.width();

                for index in 0..width {
                    // Get the next terminal cell at the given position
                    match iter.next() {
                        // Stop if we're at the end of the buffer
                        None => return,
                        // Set the cell to the current grapheme
                        Some((_, cell)) => {
                            if let Some(y) = self.text_y.get_mut(at) {
                                *y = Some(origin.y);
                            }
                            at += 1;
                            let next = Grapheme {
                                // Create a new shared reference to the text
                                color,
                                index,
                                width,
                                // Export the set of unicode code points for this graphene into an UTF-8 string
                                char: grapheme.to_string(),
                            };

                            if match cell.grapheme {
                                None => true,
                                Some(ref previous) => {
                                    previous.color != next.color || previous.char != next.char
                                }
                            } {
                                cell.grapheme = Some(Rc::new(next))
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Standard base64 with padding (for OSC 52).
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);

    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));

        for i in 0..4 {
            out.push(if i <= chunk.len() { TABLE[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"https://a.org/?q=1"), "aHR0cHM6Ly9hLm9yZy8/cT0x");
    }

    use super::*;

    /// Resizing a window down to one or two cells made the navigation bar
    /// rectangle wider than the buffer and panicked the render thread.
    #[test]
    fn fill_rect_is_clipped_to_tiny_viewports() {
        let red = Color::new(255, 0, 0);
        for (w, h) in [(1, 1), (2, 2), (1, 40), (40, 1), (3, 8)] {
            let mut r = Renderer::new();
            r.set_size(Size::new(w, h));
            r.fill_rect(Rect::new(0, 0, 80, 1), red);
            r.fill_rect(Rect::new(-5, -5, 200, 200), red);
            r.fill_rect(Rect::new(500, 500, 10, 10), red);
            assert!(r.cells.iter().all(|(_, c)| c.quadrant.0 == red));
        }
    }

    #[test]
    fn fill_rect_inside_viewport_is_unchanged() {
        let (red, blue) = (Color::new(255, 0, 0), Color::new(0, 0, 255));
        let mut r = Renderer::new();
        r.set_size(Size::new(10, 5));
        r.fill_rect(Rect::new(0, 0, 10, 6), blue);
        r.fill_rect(Rect::new(2, 1, 3, 2), red);
        let red_cells: Vec<usize> = r.cells.iter().enumerate()
            .filter(|(_, (_, c))| c.quadrant.0 == red).map(|(i, _)| i).collect();
        assert_eq!(red_cells, vec![12, 13, 14, 22, 23, 24]);
    }

    /// A click on a cell of drawn page text goes just above that text's
    /// baseline (its painted place), not to the cell's geometric middle.
    #[test]
    fn drawn_text_cells_remember_where_the_text_was_painted() {
        let mut r = Renderer::new();
        r.set_layout(Size::new(20, 10), 1);
        // Baseline at device row 9: drawn on page row (9 + 1) / 4 = 2, terminal row 3.
        r.draw_text("Hi", Point::new(4, 9), Size::new(0, 0), Color::new(1, 2, 3));
        assert_eq!(r.click_y(2, 3), Some(8));
        assert_eq!(r.click_y(3, 3), Some(8));
        assert_eq!(r.click_y(4, 3), None, "past the text");
        assert_eq!(r.click_y(2, 2), None, "the row above");
        assert_eq!(r.click_y(500, 500), None, "outside the buffer");
        r.clear_text();
        assert_eq!(r.click_y(2, 3), None, "cleared with the text");
    }

    #[test]
    fn zero_width_viewport_draws_nothing() {
        let mut r = Renderer::new();
        r.fill_rect(Rect::new(0, 0, 10, 10), Color::new(1, 2, 3));
        assert!(r.cells.is_empty());
    }
}

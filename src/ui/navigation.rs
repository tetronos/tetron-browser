use std::{env, path::PathBuf};

use unicode_width::UnicodeWidthStr;

use crate::{
    gfx::{Color, Point, Size},
    input::Key,
    utils::log,
};

pub enum NavigationAction {
    Ignore,
    Forward,
    GoTo(String),
    GoBack(),
    GoForward(),
    Refresh(),
}

#[derive(Debug)]
pub struct NavigationElement {
    pub text: String,
    pub background: Color,
    pub foreground: Color,
}

/// The bar's buttons, left to right, then the URL field.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Button {
    Back,
    Forward,
    Reload,
    Home,
    Bookmark,
    ZoomOut,
    ZoomReset,
    ZoomIn,
}

/// Page zoom steps in percent.
const ZOOM_STEPS: [u32; 13] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];

/// Script that sets the page zoom (CSS `zoom` on the root element). Chromium
/// runs a `javascript:` URL in the current page without navigating.
fn zoom_script(percent: u32) -> String {
    format!("javascript:void(document.documentElement.style.zoom='{percent}%')")
}

pub struct Navigation {
    url: Option<String>,
    title: String,
    /// Resolved on first use: resolving can write the default bookmarks page.
    home: Option<String>,
    /// The bookmarks file, resolved on first use like `home`.
    bookmarks: Option<Option<PathBuf>>,
    bookmarked: bool,
    zoom: u32,
    size: Size,
    cursor: Option<usize>,
    can_go_back: bool,
    can_go_forward: bool,
}

impl Navigation {
    pub fn new() -> Self {
        Self {
            url: None,
            title: String::new(),
            home: None,
            bookmarks: None,
            bookmarked: false,
            zoom: 100,
            size: (0, 0).into(),
            cursor: None,
            can_go_back: false,
            can_go_forward: false,
        }
    }

    /// Geometry shared by `render` and `mouse_down`: each button with its
    /// first column, label and enabled state, and the URL text column.
    fn layout(&self) -> (Vec<(Button, i32, String, bool)>, usize) {
        let zoom = format!("{:>3}%", self.zoom);
        let buttons = [
            (Button::Back, "\u{276e}", self.can_go_back),
            (Button::Forward, "\u{276f}", self.can_go_forward),
            (Button::Reload, "↻", true),
            (Button::Home, "\u{2302}", true),
            (Button::Bookmark, if self.bookmarked { "★" } else { "☆" }, self.url.is_some()),
            (Button::ZoomOut, "−", self.zoom > ZOOM_STEPS[0]),
            (Button::ZoomReset, zoom.as_str(), self.zoom != 100),
            (Button::ZoomIn, "+", self.zoom < ZOOM_STEPS[ZOOM_STEPS.len() - 1]),
        ];
        let mut x = 0;
        let mut out = Vec::new();

        for (button, label, enabled) in buttons {
            out.push((button, x, label.to_owned(), enabled));
            x += label.width() as i32 + 2;
        }

        // `[` and a space before the URL text.
        (out, x as usize + 2)
    }

    fn url_column(&self) -> usize {
        self.layout().1
    }

    fn bookmarks_path(&mut self) -> Option<PathBuf> {
        self.bookmarks
            .get_or_insert_with(crate::home::bookmarks_file)
            .clone()
    }

    fn refresh_bookmarked(&mut self) {
        self.bookmarked = match (self.bookmarks_path(), &self.url) {
            (Some(path), Some(url)) => std::fs::read_to_string(path)
                .map(|text| crate::bookmarks::contains(&text, url))
                .unwrap_or(false),
            _ => false,
        };
    }

    fn click(&mut self, button: Button) -> NavigationAction {
        let zoom_to = |zoom: u32| NavigationAction::GoTo(zoom_script(zoom));

        match button {
            Button::Back => NavigationAction::GoBack(),
            Button::Forward => NavigationAction::GoForward(),
            Button::Reload => NavigationAction::Refresh(),
            Button::Home => NavigationAction::GoTo(self.home.get_or_insert_with(crate::home::url).clone()),
            Button::Bookmark => {
                if let (Some(path), Some(url)) = (self.bookmarks_path(), self.url.clone()) {
                    match crate::bookmarks::toggle(&path, &self.title, &url) {
                        Ok(marked) => self.bookmarked = marked,
                        Err(err) => log::warning!("bookmark {url} in {}: {err}", path.display()),
                    }
                }

                NavigationAction::Ignore
            }
            Button::ZoomOut => match ZOOM_STEPS.iter().rev().find(|&&z| z < self.zoom) {
                Some(&zoom) => {
                    self.zoom = zoom;
                    zoom_to(zoom)
                }
                None => NavigationAction::Ignore,
            },
            Button::ZoomIn => match ZOOM_STEPS.iter().find(|&&z| z > self.zoom) {
                Some(&zoom) => {
                    self.zoom = zoom;
                    zoom_to(zoom)
                }
                None => NavigationAction::Ignore,
            },
            Button::ZoomReset => {
                self.zoom = 100;
                zoom_to(100)
            }
        }
    }

    pub fn cursor(&self) -> Option<Point> {
        Some((self.url_column() as i32 + self.cursor? as i32, 0).into())
    }

    pub fn keypress(&mut self, key: &Key) -> NavigationAction {
        let modifier_key = match env::consts::OS {
            "macos" => key.modifiers.meta,
            _ => key.modifiers.alt,
        };

        match self.cursor {
            None => match (modifier_key, key.char) {
                (true, 0x14) => NavigationAction::GoBack(),
                (true, 0x13) => NavigationAction::GoForward(),
                _ => NavigationAction::Forward,
            },
            Some(cursor) => {
                if let Some(url) = &mut self.url {
                    // TODO: Unicode
                    match key.char {
                        // Return
                        0x0d => return NavigationAction::GoTo(url.clone()),
                        // Up
                        0x11 => self.cursor = Some(0),
                        // Down
                        0x12 => self.cursor = Some(url.width()),
                        // Right
                        0x13 => self.cursor = Some((cursor + 1).min(url.width())),
                        // Left
                        0x14 => self.cursor = Some(if cursor > 0 { cursor - 1 } else { 0 }),
                        // Backspace
                        0x7f => {
                            if cursor > 0 {
                                url.remove(cursor - 1);

                                self.cursor = Some(cursor - 1);
                            }
                        }
                        key => {
                            url.insert(cursor, key as char);

                            self.cursor = Some((cursor + 1).min(url.width()))
                        }
                    }

                    NavigationAction::Ignore
                } else {
                    NavigationAction::Forward
                }
            }
        }
    }

    pub fn display_url(&self) -> &str {
        match &self.url {
            None => "about:blank",
            Some(url) => url,
        }
    }

    pub fn url_size(&self) -> usize {
        self.display_url().width()
    }

    pub fn mouse_up(&mut self, origin: Point) -> NavigationAction {
        if origin.y != 0 {
            self.cursor = None;

            NavigationAction::Forward
        } else {
            NavigationAction::Ignore
        }
    }
    pub fn mouse_down(&mut self, origin: Point) -> NavigationAction {
        if origin.y != 0 {
            self.cursor = None;

            return NavigationAction::Forward;
        }

        self.cursor = None;

        let (buttons, url_column) = self.layout();
        let x = origin.x;

        if x >= 0 && x as usize >= url_column {
            self.cursor = Some(self.url_size().min(x as usize - url_column));

            log::debug!("setting cursor to {:?}", self.cursor);

            return NavigationAction::Ignore;
        }

        match buttons
            .iter()
            .find(|(_, start, label, _)| x >= *start && x < start + label.width() as i32 + 2)
        {
            Some(&(button, _, _, true)) => self.click(button),
            _ => NavigationAction::Ignore,
        }
    }
    pub fn mouse_move(&mut self, _origin: Point) -> NavigationAction {
        NavigationAction::Forward
    }

    pub fn push(&mut self, url: &str, can_go_back: bool, can_go_forward: bool) {
        if match (self.cursor, &self.url) {
            (None, _) => false,
            (_, None) => true,
            (_, Some(current)) => current != url,
        } {
            self.cursor = Some(url.len())
        }

        // A new page starts unzoomed (the zoom is a style on the old page).
        if self.url.as_deref() != Some(url) {
            self.zoom = 100;
        }

        self.url = Some(url.to_owned());
        self.can_go_back = can_go_back;
        self.can_go_forward = can_go_forward;
        self.refresh_bookmarked();
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_owned();
    }

    pub fn set_size(&mut self, size: Size) {
        self.size = size
    }

    pub fn render_btn(&self, icon: &str, enabled: bool) -> [NavigationElement; 3] {
        let background = Color::splat(255);
        let foreground = Color::splat(0);

        [
            NavigationElement {
                text: "[".to_owned(),
                background,
                foreground,
            },
            NavigationElement {
                text: icon.to_owned(),
                background,
                foreground: if enabled {
                    foreground
                } else {
                    Color::splat(200)
                },
            },
            NavigationElement {
                text: "]".to_owned(),
                background,
                foreground,
            },
        ]
    }

    pub fn render(&self, size: Size) -> Vec<(Point, NavigationElement)> {
        let (buttons, url_column) = self.layout();
        let ui_elements = url_column as u32 + 2;
        let space = if size.width >= ui_elements {
            (size.width - ui_elements) as usize
        } else {
            0
        };
        let url: String = self.display_url().chars().take(space).collect();
        let width = url.width();
        let padded = format!(" {}{} ", url, " ".repeat(space - width));
        let mut elements = Vec::new();
        let mut point = Point::splat(0);

        let lists = buttons
            .iter()
            .map(|(_, _, label, enabled)| self.render_btn(label, *enabled))
            .chain([self.render_btn(&padded, true)]);

        for list in lists {
            for element in list {
                let width = element.text.width() as i32;

                elements.push((point.clone(), element));

                point = point + (width, 0);
            }
        }

        elements
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav() -> Navigation {
        let mut nav = Navigation::new();
        nav.home = Some("file:///home/u/.config/tetron-browser/bookmarks.html".into());
        nav.bookmarks = Some(None);
        nav.push("https://example.com", true, false);
        nav
    }

    fn bar(nav: &Navigation, width: u32) -> String {
        nav.render((width, 1).into()).iter().map(|(_, e)| e.text.as_str()).collect()
    }

    /// The first column of the button whose label is `label`, from the drawn bar.
    fn column_of(nav: &Navigation, label: &str) -> i32 {
        let text = bar(nav, 80);
        text[..text.find(label).unwrap()].width() as i32
    }

    #[test]
    fn bar_lists_every_button_then_the_url_and_fills_the_width() {
        let nav = nav();
        let text = bar(&nav, 80);

        assert!(text.starts_with("[\u{276e}][\u{276f}][↻][\u{2302}][☆][−][100%][+][ https://example.com"), "{text}");
        assert_eq!(text.width(), 80);
        assert_eq!(nav.url_column(), 29);
    }

    #[test]
    fn clicks_hit_the_button_drawn_there() {
        let mut nav = nav();

        let home = column_of(&nav, "[\u{2302}]");
        for x in home..home + 3 {
            match nav.mouse_down((x, 0).into()) {
                NavigationAction::GoTo(url) => assert!(url.ends_with("/bookmarks.html")),
                _ => panic!("column {x} is not the home button"),
            }
        }
        let reload = column_of(&nav, "[↻]") + 1;
        assert!(matches!(nav.mouse_down((reload, 0).into()), NavigationAction::Refresh()));
    }

    #[test]
    fn zoom_steps_reset_and_new_pages_start_at_100() {
        let mut nav = nav();
        let plus = column_of(&nav, "[+]") + 1;

        match nav.mouse_down((plus, 0).into()) {
            NavigationAction::GoTo(js) => assert_eq!(js, zoom_script(110)),
            _ => panic!("zoom in did nothing"),
        }
        nav.mouse_down((plus, 0).into());
        assert!(bar(&nav, 80).contains("[125%]"));

        let reset = column_of(&nav, "[125%]") + 2;
        assert!(matches!(nav.mouse_down((reset, 0).into()), NavigationAction::GoTo(_)));
        assert_eq!(nav.zoom, 100);

        let minus = column_of(&nav, "[−]") + 1;
        for _ in 0..20 {
            nav.mouse_down((minus, 0).into());
        }
        assert_eq!(nav.zoom, 50);
        assert!(bar(&nav, 80).contains("[ 50%]"));

        nav.push("https://example.com", true, false);
        assert_eq!(nav.zoom, 50);
        nav.push("https://other.example", true, false);
        assert_eq!(nav.zoom, 100);
    }

    #[test]
    fn bookmark_button_toggles_the_page_in_the_file() {
        let dir = std::env::temp_dir().join(format!("tetron-browser-nav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bookmarks.html");
        std::fs::write(&path, crate::home::DEFAULT_BOOKMARKS).unwrap();

        let mut nav = nav();
        nav.bookmarks = Some(Some(path.clone()));
        nav.set_title("Example Domain");
        nav.push("https://example.com/page", true, false);
        let star = column_of(&nav, "[☆]") + 1;

        nav.mouse_down((star, 0).into());
        assert!(bar(&nav, 80).contains("[★]"));
        assert!(std::fs::read_to_string(&path).unwrap().contains(">Example Domain</A>"));

        nav.push("https://example.com/page", true, false);
        assert!(nav.bookmarked, "state survives a reload of the same page");

        nav.mouse_down((star, 0).into());
        assert!(!nav.bookmarked);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("example.com/page"));
    }

    #[test]
    fn clicking_the_url_puts_the_cursor_under_the_click() {
        let mut nav = nav();
        let column = nav.url_column() as i32;

        nav.mouse_down((column + 8, 0).into());
        assert_eq!(nav.cursor(), Some((column + 8, 0).into()));
        nav.mouse_down((column - 2, 0).into());
        assert_eq!(nav.cursor(), None);
    }
}

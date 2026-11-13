use std::{env, path::PathBuf};

use unicode_width::UnicodeWidthStr;

use crate::{
    gfx::{Color, Point, Size},
    input::Key,
    utils::log,
};

use super::{
    menu::{Entry, Menu, Probe},
    tabs::{session_path, Tabs},
};

pub enum NavigationAction {
    Ignore,
    Forward,
    GoTo(String),
    /// Several loads in order (a scroll report on the old page, then the new page).
    Run(Vec<String>),
    /// Middle click on the page at this cell: open the link there in a
    /// background tab.
    OpenLinkAt(Point),
    /// Right click on the page at this cell: probe for a link or image there;
    /// the answer opens the context menu (see `set_title`).
    ProbeAt(Point),
    /// Put this text on the clipboard (OSC 52).
    Copy(String),
    /// Download `url` with the page `referer` into the download directory.
    Save { url: String, referer: Option<String>, image: bool },
    GoBack(),
    GoForward(),
    Refresh(),
}

/// Start of a page title that carries a message from one of our scripts
/// instead of a title (scripts can not return values to the browser).
/// Plain text: Chromium strips control characters from titles.
const TITLE_MESSAGE: &str = "tetron-browser-message:";

/// Environment variable the launcher sets when it opened the saved active tab.
pub const ENV_RESTORED: &str = "TETRON_BROWSER_RESTORED";

/// Script that reports the page's scroll position for tab `id`.
fn scroll_report_script(id: u64) -> String {
    format!("javascript:void(document.title='tetron-browser-message:scroll {id} '+Math.round(window.scrollY))")
}

/// Script that scrolls to `y` once the page has loaded.
fn scroll_restore_script(y: i64) -> String {
    format!(
        "javascript:void((function(){{function s(){{window.scrollTo(0,{y})}}\
         if(document.readyState=='complete')s();else window.addEventListener('load',s)}})())"
    )
}

/// Script that reports the link and the image at CSS pixel (x, y): the
/// message `probe <link> <image>`, each URI-component encoded, `-` for none.
/// Image: an `img` (its `currentSrc`), a `video` poster, or the element's
/// CSS background image. URLs longer than a title can carry are left out.
/// No `%` or `#` in the script: Chromium decodes and cuts `javascript:` URLs.
pub fn probe_script(x: i32, y: i32) -> String {
    format!(
        "javascript:void((function(){{var e=document.elementFromPoint({x},{y});if(!e)return;\
         var a=e.closest('a[href]'),i='';\
         if(e.tagName=='IMG')i=e.currentSrc||e.src;\
         else if(e.tagName=='VIDEO')i=e.poster||'';\
         else{{var m=/url[(][\"']?(.*?)[\"']?[)]/.exec(getComputedStyle(e).backgroundImage);\
         if(m)i=new URL(m[1],document.baseURI).href}}\
         function f(u){{u=u?encodeURIComponent(u):'';return u&&u.length<1800?u:'-'}}\
         var t=document.title;\
         document.title='tetron-browser-message:probe '+f(a&&a.href)+' '+f(i);\
         setTimeout(function(){{document.title=t}},100)}})())"
    )
}

/// Script that finds the link at CSS pixel (x, y) and reports its address.
pub fn open_link_script(x: i32, y: i32) -> String {
    format!(
        "javascript:void((function(){{var e=document.elementFromPoint({x},{y}),\
         a=e&&e.closest('a[href]');if(!a)return;var t=document.title;\
         document.title='tetron-browser-message:open '+a.href;\
         setTimeout(function(){{document.title=t}},100)}})())"
    )
}

/// What a click on the tab strip hits.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StripHit {
    Tab(usize),
    Close(usize),
    New,
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
    /// Opened on first use: loading reads and writes the session file.
    tabs: Option<Tabs>,
    /// Address and scroll to restore when the switched-to tab's page arrives.
    pending_restore: Option<(String, i64)>,
    /// UI rows the cell buffer has now (1, or 2 with the tab strip).
    rows: u32,
    size: Size,
    cursor: Option<usize>,
    can_go_back: bool,
    can_go_forward: bool,
    /// The context menu, while it is open.
    menu: Option<Menu>,
    /// The right-clicked cell waiting for its probe answer.
    menu_at: Option<Point>,
    /// The mouse up of a click the menu took: not for the page.
    swallow_up: bool,
    /// A short message shown in place of the URL ("copied", "saved to ...").
    status: Option<String>,
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
            tabs: None,
            pending_restore: None,
            rows: 1,
            size: (0, 0).into(),
            cursor: None,
            can_go_back: false,
            can_go_forward: false,
            menu: None,
            menu_at: None,
            swallow_up: false,
            status: None,
        }
    }

    /// The whole cell buffer (UI rows and page) and its UI rows, for the menu.
    fn buffer_size(&self) -> (Size, u32) {
        (Size::new(self.size.width, self.size.height + self.rows), self.rows)
    }

    pub fn set_status(&mut self, status: Option<String>) {
        self.status = status;
    }

    /// Runs a menu entry.
    fn run(&mut self, entry: Entry) -> NavigationAction {
        match entry {
            Entry::OpenInNewTab(url) => {
                self.tabs().open_background(&url);
                NavigationAction::Ignore
            }
            Entry::CopyLink(url) | Entry::CopyImage(url) => NavigationAction::Copy(url),
            Entry::SaveLink(url) => {
                self.status = Some(format!("saving {url}"));
                NavigationAction::Save { url, referer: self.url.clone(), image: false }
            }
            Entry::SaveImage(url) => {
                self.status = Some(format!("saving {url}"));
                NavigationAction::Save { url, referer: self.url.clone(), image: true }
            }
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

        if let Some(mut menu) = self.menu.take() {
            match key.char {
                // Up, Down
                0x11 => menu.step(-1),
                0x12 => menu.step(1),
                // Return
                0x0d => return menu.hover().and_then(|h| menu.entry(h).cloned()).map_or(NavigationAction::Ignore, |e| self.run(e)),
                // Anything else closes the menu.
                _ => return NavigationAction::Ignore,
            }

            self.menu = Some(menu);
            return NavigationAction::Ignore;
        }

        self.status = None;

        match self.cursor {
            None => match (modifier_key, key.char) {
                (true, 0x14) => NavigationAction::GoBack(),
                (true, 0x13) => NavigationAction::GoForward(),
                // Alt+1..9 switch tabs, Alt+W closes the current one.
                _ if key.modifiers.alt && (b'1'..=b'9').contains(&key.char) => {
                    self.switch_to((key.char - b'1') as usize)
                }
                _ if key.modifiers.alt && key.char.to_ascii_lowercase() == b'w' => {
                    let active = self.tabs().active();
                    self.close_tab(active)
                }
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

    fn on_page(&self, origin: Point) -> bool {
        origin.y >= self.rows as i32
    }

    /// `button`: 0 left, 1 middle, 2 right. Only left clicks reach the page;
    /// middle clicks open links in background tabs, right clicks open the
    /// context menu.
    pub fn mouse_up(&mut self, origin: Point, button: u8) -> NavigationAction {
        if button == 0 && std::mem::take(&mut self.swallow_up) {
            return NavigationAction::Ignore;
        }

        if self.on_page(origin) && button == 0 {
            self.cursor = None;

            NavigationAction::Forward
        } else {
            NavigationAction::Ignore
        }
    }
    pub fn mouse_down(&mut self, origin: Point, button: u8) -> NavigationAction {
        self.status = None;
        self.menu_at = None;

        // An open menu takes a left click: on an entry it runs it, elsewhere
        // it only closes the menu. Other buttons close it and go on.
        if let Some(menu) = self.menu.take() {
            if button == 0 {
                let (size, top) = self.buffer_size();
                self.swallow_up = true;

                return match menu.hit(origin, size, top).and_then(|i| menu.entry(i).cloned()) {
                    Some(entry) => self.run(entry),
                    None => NavigationAction::Ignore,
                };
            }
        }

        if self.on_page(origin) {
            self.cursor = None;

            return match button {
                0 => NavigationAction::Forward,
                1 => NavigationAction::OpenLinkAt(origin),
                _ => {
                    self.menu_at = Some(origin);
                    NavigationAction::ProbeAt(origin)
                }
            };
        }

        if button != 0 {
            return NavigationAction::Ignore;
        }

        self.cursor = None;

        if origin.y == 1 {
            let hit = self
                .strip_layout(self.size.width)
                .into_iter()
                .find(|(_, start, text, _)| origin.x >= *start && origin.x < start + text.width() as i32);

            return match hit.map(|(hit, ..)| hit) {
                Some(StripHit::Tab(index)) => self.switch_to(index),
                Some(StripHit::Close(index)) => self.close_tab(index),
                Some(StripHit::New) => {
                    let save = self.scroll_report();
                    let home = self.home.get_or_insert_with(crate::home::url).clone();

                    self.tabs().open_active(&home);
                    NavigationAction::Run(save.into_iter().chain([home]).collect())
                }
                None => NavigationAction::Ignore,
            };
        }

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
    pub fn mouse_move(&mut self, origin: Point) -> NavigationAction {
        let (size, top) = self.buffer_size();

        if let Some(menu) = &mut self.menu {
            menu.set_hover(menu.hit(origin, size, top));
            return NavigationAction::Ignore;
        }

        if self.on_page(origin) {
            NavigationAction::Forward
        } else {
            NavigationAction::Ignore
        }
    }

    fn tabs(&mut self) -> &mut Tabs {
        if self.tabs.is_none() {
            let restored = env::var_os(ENV_RESTORED).is_some();
            let tabs = Tabs::load(session_path(), restored);

            // Started on the saved active tab: scroll back to where it was.
            if restored {
                if let Some(tab) = tabs.list().get(tabs.active()) {
                    self.pending_restore = Some((tab.url.clone(), tab.scroll));
                }
            }

            self.tabs = Some(tabs);
        }

        self.tabs.as_mut().unwrap()
    }

    /// UI rows wanted above the page: the tab strip shows with 2+ tabs.
    pub fn ui_rows(&mut self) -> u32 {
        if self.tabs().len() > 1 {
            2
        } else {
            1
        }
    }

    /// The scroll report for the tab being left, when there is one.
    fn scroll_report(&mut self) -> Option<String> {
        self.tabs().active_id().map(scroll_report_script)
    }

    fn switch_to(&mut self, index: usize) -> NavigationAction {
        let save = self.scroll_report();

        match self.tabs().activate(index) {
            Some((url, scroll)) => {
                self.pending_restore = Some((url.clone(), scroll));
                NavigationAction::Run(save.into_iter().chain([url]).collect())
            }
            None => NavigationAction::Ignore,
        }
    }

    fn close_tab(&mut self, index: usize) -> NavigationAction {
        match self.tabs().close(index) {
            Some((url, scroll)) => {
                self.pending_restore = Some((url.clone(), scroll));
                NavigationAction::GoTo(url)
            }
            None => NavigationAction::Ignore,
        }
    }

    /// A page arrived. Returns a script to run in it: the scroll position to
    /// restore after a tab switch.
    pub fn push(&mut self, url: &str, can_go_back: bool, can_go_forward: bool) -> Option<String> {
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
            self.menu = None;
            self.menu_at = None;
        }

        self.url = Some(url.to_owned());
        self.can_go_back = can_go_back;
        self.can_go_forward = can_go_forward;
        self.refresh_bookmarked();
        self.tabs().on_page(url);

        let same = |a: &str| a.strip_suffix('/').unwrap_or(a) == url.strip_suffix('/').unwrap_or(url);

        match self.pending_restore.take() {
            Some((target, y)) if same(&target) => (y > 0).then(|| scroll_restore_script(y)),
            // Not the switched-to page yet (the old page can report first).
            other => {
                self.pending_restore = other;
                None
            }
        }
    }

    /// A page title, or a message from one of our scripts (see
    /// `TITLE_MESSAGE`). Returns whether it is a real title to show.
    pub fn set_title(&mut self, title: &str) -> bool {
        if let Some(message) = title.strip_prefix(TITLE_MESSAGE) {
            if let Some(url) = message.strip_prefix("open ") {
                self.tabs().open_background(url.trim());
            } else if let Some(body) = message.strip_prefix("probe ") {
                let entries = Probe::parse(body).entries();

                match self.menu_at.take() {
                    Some(at) if !entries.is_empty() => self.menu = Some(Menu::new(entries, at)),
                    _ => (),
                }
            } else if let Some(rest) = message.strip_prefix("scroll ") {
                let mut parts = rest.split_whitespace().map(str::parse::<i64>);

                if let (Some(Ok(id)), Some(Ok(y))) = (parts.next(), parts.next()) {
                    self.tabs().set_scroll(id as u64, y);
                }
            }

            return false;
        }

        self.title = title.to_owned();
        self.tabs().on_title(title);
        true
    }

    /// The page size in cells and the UI rows the cell buffer has above it.
    pub fn set_size(&mut self, size: Size, rows: u32) {
        self.size = size;
        self.rows = rows;
    }

    /// The tab strip (row 1), shared by `render` and `mouse_down`: each part
    /// with its first column, text and whether it is the active tab.
    fn strip_layout(&self, width: u32) -> Vec<(StripHit, i32, String, bool)> {
        let Some(tabs) = &self.tabs else { return Vec::new() };
        let count = tabs.len().max(1);
        // Each tab is `[N title ×]`; the rest of the row is shared out.
        let each = ((width as usize).saturating_sub(3) / count).clamp(5, 28);
        let mut out = Vec::new();
        let mut x = 0;

        for (index, tab) in tabs.list().iter().enumerate() {
            let name = if tab.title.trim().is_empty() { tab.url.as_str() } else { tab.title.trim() };
            let room = each - 4;
            let mut label = format!("{} {name}", index + 1);

            if label.width() > room {
                label = label.chars().take(room.saturating_sub(1)).collect::<String>() + "…";
            }

            let body = format!("[{label:<room$} ");
            let active = index == tabs.active();
            let width = body.width() as i32;

            out.push((StripHit::Tab(index), x, body, active));
            out.push((StripHit::Close(index), x + width, "×".to_owned(), active));
            out.push((StripHit::Tab(index), x + width + 1, "]".to_owned(), active));
            x += width + 2;
        }

        out.push((StripHit::New, x, "[+]".to_owned(), false));
        out
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
        let shown = match (&self.status, self.cursor) {
            (Some(status), None) => status.as_str(),
            _ => self.display_url(),
        };
        let url: String = shown.chars().take(space).collect();
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

        if self.rows > 1 {
            let light = Color::splat(255);
            let dark = Color::new(40, 40, 40);
            let mut end = 0;

            for (hit, x, text, active) in self.strip_layout(size.width) {
                let (background, foreground) = match (active, hit) {
                    (true, StripHit::Close(_)) => (dark, Color::splat(200)),
                    (true, _) => (dark, light),
                    (false, StripHit::Close(_)) => (light, Color::splat(120)),
                    (false, _) => (light, Color::splat(0)),
                };

                end = x + text.width() as i32;
                elements.push((Point::new(x, 1), NavigationElement { text, background, foreground }));
            }

            // Clear the rest of the row.
            let rest = (size.width as i32 - end).max(0) as usize;
            elements.push((
                Point::new(end, 1),
                NavigationElement { text: " ".repeat(rest), background: light, foreground: light },
            ));
        }

        elements
    }

    /// What is drawn over the page: the context menu.
    pub fn render_overlay(&self) -> Vec<(Point, NavigationElement)> {
        let (size, top) = self.buffer_size();

        self.menu.as_ref().map_or_else(Vec::new, |menu| menu.render(size, top))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav() -> Navigation {
        let mut nav = Navigation::new();
        nav.home = Some("file:///home/u/.config/tetron-browser/bookmarks.html".into());
        nav.bookmarks = Some(None);
        nav.tabs = Some(Tabs::new(None));
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
            match nav.mouse_down((x, 0).into(), 0) {
                NavigationAction::GoTo(url) => assert!(url.ends_with("/bookmarks.html")),
                _ => panic!("column {x} is not the home button"),
            }
        }
        let reload = column_of(&nav, "[↻]") + 1;
        assert!(matches!(nav.mouse_down((reload, 0).into(), 0), NavigationAction::Refresh()));
    }

    #[test]
    fn zoom_steps_reset_and_new_pages_start_at_100() {
        let mut nav = nav();
        let plus = column_of(&nav, "[+]") + 1;

        match nav.mouse_down((plus, 0).into(), 0) {
            NavigationAction::GoTo(js) => assert_eq!(js, zoom_script(110)),
            _ => panic!("zoom in did nothing"),
        }
        nav.mouse_down((plus, 0).into(), 0);
        assert!(bar(&nav, 80).contains("[125%]"));

        let reset = column_of(&nav, "[125%]") + 2;
        assert!(matches!(nav.mouse_down((reset, 0).into(), 0), NavigationAction::GoTo(_)));
        assert_eq!(nav.zoom, 100);

        let minus = column_of(&nav, "[−]") + 1;
        for _ in 0..20 {
            nav.mouse_down((minus, 0).into(), 0);
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

        nav.mouse_down((star, 0).into(), 0);
        assert!(bar(&nav, 80).contains("[★]"));
        assert!(std::fs::read_to_string(&path).unwrap().contains(">Example Domain</A>"));

        nav.push("https://example.com/page", true, false);
        assert!(nav.bookmarked, "state survives a reload of the same page");

        nav.mouse_down((star, 0).into(), 0);
        assert!(!nav.bookmarked);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("example.com/page"));
    }

    #[test]
    fn clicking_the_url_puts_the_cursor_under_the_click() {
        let mut nav = nav();
        let column = nav.url_column() as i32;

        nav.mouse_down((column + 8, 0).into(), 0);
        assert_eq!(nav.cursor(), Some((column + 8, 0).into()));
        nav.mouse_down((column - 2, 0).into(), 0);
        assert_eq!(nav.cursor(), None);
    }

    fn with_tabs() -> Navigation {
        let mut nav = nav();
        nav.set_title("Example");
        nav.tabs().open_background("https://b.org/");
        nav.tabs().open_background("https://c.org/");
        let rows = nav.ui_rows();
        nav.set_size((80, 20).into(), rows);
        nav
    }

    #[test]
    fn strip_shows_with_two_tabs_and_clicks_switch_and_close() {
        let mut nav = nav();
        assert_eq!(nav.ui_rows(), 1);
        assert!(nav.render((80, 1).into()).iter().all(|(p, _)| p.y == 0));

        let mut nav = with_tabs();
        assert_eq!(nav.ui_rows(), 2);
        let strip: String = nav
            .render((80, 2).into())
            .iter()
            .filter(|(p, _)| p.y == 1)
            .map(|(_, e)| e.text.as_str())
            .collect();
        assert!(strip.starts_with("[1 Example"), "{strip}");
        assert!(strip.contains("[2 https://b.org/"), "{strip}");
        assert!(strip.contains("×][+]"), "{strip}");
        assert_eq!(strip.width(), 80);

        let parts = nav.strip_layout(80);
        let at = |hit: StripHit| parts.iter().find(|p| p.0 == hit).unwrap().1;

        match nav.mouse_down((at(StripHit::Tab(2)) + 2, 1).into(), 0) {
            NavigationAction::Run(urls) => {
                assert!(urls[0].contains("tetron-browser-message:scroll"), "{urls:?}");
                assert_eq!(urls[1], "https://c.org/");
            }
            _ => panic!("tab click did not switch"),
        }
        assert_eq!(nav.tabs().active(), 2);

        assert!(matches!(nav.mouse_down((at(StripHit::Close(1)), 1).into(), 0), NavigationAction::Ignore));
        assert_eq!(nav.tabs().len(), 2);
        assert!(matches!(nav.mouse_down((at(StripHit::New), 1).into(), 0), NavigationAction::Ignore),
            "the strip moved after the close; the old [+] column is now empty");
    }

    #[test]
    fn right_click_probes_middle_opens_links_left_goes_through() {
        let mut nav = with_tabs();

        assert!(matches!(nav.mouse_down((10, 5).into(), 0), NavigationAction::Forward));
        assert!(matches!(nav.mouse_down((10, 5).into(), 2), NavigationAction::ProbeAt(p) if p == (10, 5).into()));
        assert!(matches!(nav.mouse_down((10, 5).into(), 1), NavigationAction::OpenLinkAt(_)));
        assert!(matches!(nav.mouse_up((10, 5).into(), 2), NavigationAction::Ignore));
        assert!(matches!(nav.mouse_up((10, 5).into(), 0), NavigationAction::Forward));
        assert!(matches!(nav.mouse_down((10, 1).into(), 2), NavigationAction::Ignore), "the strip row is not page");
    }

    #[test]
    fn script_messages_open_tabs_and_store_scroll_and_are_not_titles() {
        let mut nav = nav();

        assert!(!nav.set_title("tetron-browser-message:open https://d.org/x"));
        assert_eq!(nav.tabs().list()[1].url, "https://d.org/x");
        assert_eq!(nav.title, "", "a message is not the page title");

        let id = nav.tabs().active_id().unwrap();
        assert!(!nav.set_title(&format!("tetron-browser-message:scroll {id} 640")));
        assert_eq!(nav.tabs().list()[0].scroll, 640);
        assert!(nav.set_title("Real"));
    }

    /// Right-click, the probe answers, the menu opens at the click; clicks
    /// and keys on it run entries and do not reach the page.
    #[test]
    fn context_menu_opens_on_the_probe_answer_and_runs_entries() {
        let mut nav = nav();
        nav.set_size(Size::new(80, 23), 1);
        let probe = "tetron-browser-message:probe https%3A%2F%2Fd.org%2Ff.pdf https%3A%2F%2Fd.org%2Fi.png";

        assert!(!nav.set_title(probe), "no right-click waiting: no menu");
        assert!(nav.render_overlay().is_empty());

        assert!(matches!(nav.mouse_down((10, 5).into(), 2), NavigationAction::ProbeAt(_)));
        nav.set_title("tetron-browser-message:probe - -");
        assert!(nav.render_overlay().is_empty(), "nothing found: no menu");

        nav.mouse_down((10, 5).into(), 2);
        nav.set_title(probe);
        let overlay = nav.render_overlay();
        assert_eq!(overlay.len(), 5);
        assert_eq!(overlay[0].0, Point::new(10, 6));
        assert!(overlay[1].1.text.contains("Copy link address"));

        // Hover highlights, the page does not see the move.
        assert!(matches!(nav.mouse_move((12, 7).into()), NavigationAction::Ignore));
        assert_eq!(nav.menu.as_ref().unwrap().hover(), Some(1));

        // Click on "Copy link address": copies, and the mouse up is swallowed.
        assert!(matches!(nav.mouse_down((12, 7).into(), 0), NavigationAction::Copy(u) if u == "https://d.org/f.pdf"));
        assert!(matches!(nav.mouse_up((12, 7).into(), 0), NavigationAction::Ignore));
        assert!(matches!(nav.mouse_up((12, 7).into(), 0), NavigationAction::Forward));
        assert!(nav.render_overlay().is_empty());

        // Keyboard: Down three times to "Save link", Return saves.
        nav.mouse_down((10, 5).into(), 2);
        nav.set_title(probe);
        for _ in 0..3 {
            nav.keypress(&0x12.into());
        }
        match nav.keypress(&0x0d.into()) {
            NavigationAction::Save { url, referer, image: false } => {
                assert_eq!(url, "https://d.org/f.pdf");
                assert_eq!(referer.as_deref(), Some("https://example.com"));
            }
            _ => panic!("Return on Save link"),
        }
        assert!(bar(&nav, 120).contains("saving https://d.org/f.pdf"), "status in place of the URL");

        // Open in new tab; a click outside only closes.
        nav.mouse_down((10, 5).into(), 2);
        nav.set_title(probe);
        assert!(matches!(nav.mouse_down((40, 20).into(), 0), NavigationAction::Ignore));
        assert!(nav.render_overlay().is_empty());
        nav.mouse_up((40, 20).into(), 0);
        nav.mouse_down((10, 5).into(), 2);
        nav.set_title(probe);
        nav.mouse_down((10, 6).into(), 0);
        assert_eq!(nav.tabs().list().last().unwrap().url, "https://d.org/f.pdf");
    }

    #[test]
    fn alt_keys_switch_close_and_restore_scroll_on_arrival() {
        let mut nav = with_tabs();
        let id = nav.tabs().active_id().unwrap();
        nav.set_title(&format!("tetron-browser-message:scroll {id} 500"));
        let alt = |c: u8| Key { char: c, modifiers: crate::input::KeyModifiers { alt: true, ..Default::default() } };

        assert!(matches!(nav.keypress(&alt(b'3')), NavigationAction::Run(_)));
        assert!(nav.push("https://c.org/", true, false).is_none(), "c.org was never scrolled");
        assert!(matches!(nav.keypress(&alt(b'1')), NavigationAction::Run(_)));
        assert!(nav.push("https://b.org/", true, false).is_none(), "not the page switched to");
        let restore = nav.push("https://example.com/", true, false).unwrap();
        assert!(restore.contains("scrollTo(0,500)"), "{restore}");
        assert!(matches!(nav.keypress(&alt(b'9')), NavigationAction::Ignore));

        assert!(matches!(nav.keypress(&alt(b'w')), NavigationAction::GoTo(_)));
        assert_eq!(nav.tabs().len(), 2);
    }
}

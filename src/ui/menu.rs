//! The context menu of a right-click on a link or an image, drawn by the
//! browser over the page. One rect function serves drawing and hit-testing.

use crate::gfx::{Color, Point, Rect, Size};

use super::navigation::NavigationElement;

/// What a menu entry does with its URL.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    OpenInNewTab(String),
    OpenInApp(String),
    CopyLink(String),
    SaveLink(String),
    CopyImage(String),
    SaveImage(String),
}

impl Entry {
    fn label(&self) -> &'static str {
        match self {
            Entry::OpenInNewTab(_) => "Open in new tab",
            Entry::OpenInApp(_) => "Open in default app",
            Entry::CopyLink(_) => "Copy link address",
            Entry::SaveLink(_) => "Save link to Downloads",
            Entry::CopyImage(_) => "Copy image address",
            Entry::SaveImage(_) => "Save image to Downloads",
        }
    }
}

/// What the probe script found at the clicked pixel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub link: Option<String>,
    pub image: Option<String>,
}

impl Probe {
    /// Parses the probe message body: `<link> <image>`, each URI-component
    /// encoded, `-` for none.
    pub fn parse(body: &str) -> Probe {
        let mut fields = body.split(' ').map(|field| match field.trim() {
            "" | "-" => None,
            field => Some(percent_decode(field)).filter(|url| !url.is_empty()),
        });

        Probe { link: fields.next().flatten(), image: fields.next().flatten() }
    }

    /// The entries that apply; empty when the click hit neither.
    pub fn entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();

        if let Some(link) = &self.link {
            if crate::download::opens_in_app(link) {
                out.push(Entry::OpenInApp(link.clone()));
            }
            if !crate::download::is_external(link) {
                out.push(Entry::OpenInNewTab(link.clone()));
            }
            out.push(Entry::CopyLink(link.clone()));
            if !crate::download::is_external(link) {
                out.push(Entry::SaveLink(link.clone()));
            }
        }

        if let Some(image) = &self.image {
            out.push(Entry::CopyImage(image.clone()));
            out.push(Entry::SaveImage(image.clone()));
        }

        out
    }
}

/// Decodes `%XX` escapes (invalid ones stay as they are) as UTF-8.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);

        match (bytes[i], bytes.get(i + 1).and_then(|&b| hex(b)), bytes.get(i + 2).and_then(|&b| hex(b))) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (byte, ..) => {
                out.push(byte);
                i += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

pub struct Menu {
    entries: Vec<Entry>,
    /// The right-clicked cell (buffer coordinates, UI rows included).
    at: Point,
    hover: Option<usize>,
}

impl Menu {
    pub fn new(entries: Vec<Entry>, at: Point) -> Menu {
        Menu { entries, at, hover: None }
    }

    fn width(&self) -> u32 {
        self.entries.iter().map(|e| e.label().len()).max().unwrap_or(0) as u32 + 2
    }

    /// Where the menu is drawn inside a buffer of `size` cells whose first
    /// `top` rows are UI: below and right of the click, moved left or above
    /// when it would not fit.
    pub fn rect(&self, size: Size, top: u32) -> Rect {
        let (w, h) = (self.width() as i32, self.entries.len() as i32);
        let (width, bottom) = (size.width as i32, size.height as i32);
        let x = if self.at.x + w <= width { self.at.x } else { (width - w).max(0) };
        let y = if self.at.y + 1 + h <= bottom {
            self.at.y + 1
        } else {
            (self.at.y - h).max(top as i32)
        };

        Rect::new(x, y, w as u32, h as u32)
    }

    /// The entry at cell `point`, if the menu covers it.
    pub fn hit(&self, point: Point, size: Size, top: u32) -> Option<usize> {
        let r = self.rect(size, top);
        let (x, y) = (point.x - r.origin.x, point.y - r.origin.y);

        (x >= 0 && y >= 0 && (x as u32) < r.size.width && (y as u32) < r.size.height).then_some(y as usize)
    }

    pub fn entry(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    pub fn hover(&self) -> Option<usize> {
        self.hover
    }

    pub fn set_hover(&mut self, hover: Option<usize>) {
        self.hover = hover;
    }

    /// Keyboard: move the highlight down (`1`) or up (`-1`), wrapping.
    pub fn step(&mut self, by: i32) {
        let len = self.entries.len() as i32;

        if len > 0 {
            let from = self.hover.map_or(if by > 0 { -1 } else { len }, |h| h as i32);
            self.hover = Some((from + by).rem_euclid(len) as usize);
        }
    }

    pub fn render(&self, size: Size, top: u32) -> Vec<(Point, NavigationElement)> {
        let r = self.rect(size, top);
        let width = r.size.width as usize;

        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let (background, foreground) = if self.hover == Some(index) {
                    (Color::new(40, 40, 40), Color::splat(255))
                } else {
                    (Color::splat(230), Color::splat(0))
                };
                let text = format!(" {:<w$} ", entry.label(), w = width - 2);

                (Point::new(r.origin.x, r.origin.y + index as i32), NavigationElement { text, background, foreground })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(link: Option<&str>, image: Option<&str>) -> Probe {
        Probe { link: link.map(Into::into), image: image.map(Into::into) }
    }

    #[test]
    fn probe_message_parses_both_fields() {
        assert_eq!(Probe::parse("https%3A%2F%2Fa.org%2Fx%20y -"), probe(Some("https://a.org/x y"), None));
        assert_eq!(Probe::parse("- https%3A%2F%2Fa.org%2Fi.png"), probe(None, Some("https://a.org/i.png")));
        assert_eq!(Probe::parse("- -"), probe(None, None));
        assert_eq!(Probe::parse(""), probe(None, None));
        assert_eq!(percent_decode("a%2"), "a%2", "short escape stays");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }

    #[test]
    fn entries_follow_what_was_hit() {
        let labels = |p: Probe| p.entries().iter().map(Entry::label).collect::<Vec<_>>();

        assert_eq!(labels(probe(Some("l"), None)), ["Open in new tab", "Copy link address", "Save link to Downloads"]);
        assert_eq!(labels(probe(None, Some("i"))), ["Copy image address", "Save image to Downloads"]);
        assert_eq!(labels(probe(Some("l"), Some("i"))).len(), 5, "image inside a link");
        assert!(labels(probe(None, None)).is_empty());
        assert_eq!(labels(probe(Some("magnet:?xt=urn:btih:abc"), None)), ["Open in default app", "Copy link address"]);
        assert_eq!(labels(probe(Some("https://a.org/d.torrent"), None)), ["Open in default app", "Open in new tab", "Copy link address", "Save link to Downloads"]);
        assert_eq!(labels(probe(Some("https://a.org/news/"), None))[0], "Open in new tab", "a page: no default app");
    }

    #[test]
    fn menu_fits_inside_the_page_and_hits_what_it_draws() {
        let size = Size::new(80, 24);
        let entries = probe(Some("l"), Some("i")).entries();
        let menu = Menu::new(entries.clone(), Point::new(10, 5));
        let r = menu.rect(size, 1);
        assert_eq!((r.origin.x, r.origin.y, r.size.height), (10, 6, 5));

        for (index, (point, element)) in menu.render(size, 1).iter().enumerate() {
            assert_eq!(element.text.len() as u32, r.size.width);
            assert_eq!(menu.hit(*point, size, 1), Some(index));
            assert_eq!(menu.hit(*point + (r.size.width as i32 - 1, 0), size, 1), Some(index));
        }

        assert_eq!(menu.hit(Point::new(9, 6), size, 1), None);
        assert_eq!(menu.hit(Point::new(10, 5), size, 1), None);
        assert_eq!(menu.hit(Point::new(10, 11), size, 1), None);

        // Bottom right corner: moves left and above the click.
        let corner = Menu::new(entries.clone(), Point::new(79, 23)).rect(size, 1);
        assert_eq!((corner.origin.x as u32 + corner.size.width, corner.origin.y), (80, 18));
        // A page too short for the menu: it starts below the UI rows.
        assert_eq!(Menu::new(entries, Point::new(0, 3)).rect(Size::new(80, 5), 2).origin.y, 2);
    }

    #[test]
    fn keyboard_steps_wrap() {
        let mut menu = Menu::new(probe(Some("l"), None).entries(), Point::new(0, 1));
        menu.step(1);
        assert_eq!(menu.hover(), Some(0));
        menu.step(-1);
        assert_eq!(menu.hover(), Some(2));
        menu.step(1);
        assert_eq!(menu.hover(), Some(0));
    }
}

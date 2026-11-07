//! Light tabs: a list of pages in one browser. Only the active tab is a live
//! page; switching loads the other tab's address again and scrolls back to
//! where it was (cookies and logins stay, form input does not). One Chromium
//! serves every tab, so a tab costs no memory while it is in the background.
//!
//! The list is saved after every change to `<state>/tetron-browser/tabs.tsv`
//! (`$XDG_STATE_HOME`, else `~/.local/state`) and reopened on the next start.
//! Several browsers at once share that file: the last change wins.

use std::{env, fs, path::PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    /// Stable while the tab lives: scroll reports name the tab by id because
    /// indexes shift when tabs close.
    pub id: u64,
    pub url: String,
    pub title: String,
    /// Vertical scroll in CSS pixels, saved when the tab is left.
    pub scroll: i64,
}

#[derive(Debug)]
pub struct Tabs {
    list: Vec<Tab>,
    active: usize,
    next_id: u64,
    /// The next page shown opens as a new tab (a start with a URL, or with
    /// no saved session) instead of replacing the restored active tab.
    first_load_is_new: bool,
    path: Option<PathBuf>,
}

/// Session file location, `None` without a home directory.
pub fn session_path() -> Option<PathBuf> {
    let state = match env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(env::var_os("HOME")?).join(".local/state"),
    };

    Some(state.join("tetron-browser/tabs.tsv"))
}

/// Same page for tab matching: ignores a trailing `/`.
fn same(a: &str, b: &str) -> bool {
    a.strip_suffix('/').unwrap_or(a) == b.strip_suffix('/').unwrap_or(b)
}

impl Tabs {
    /// An empty list that is not saved anywhere (tests, no home directory).
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            list: Vec::new(),
            active: 0,
            next_id: 1,
            first_load_is_new: true,
            path,
        }
    }

    /// The saved session. `restored` says the browser was started on the
    /// saved active tab, so its first page is that tab, not a new one.
    pub fn load(path: Option<PathBuf>, restored: bool) -> Self {
        let mut tabs = Self::new(path);

        if let Some(text) = tabs.path.as_ref().and_then(|p| fs::read_to_string(p).ok()) {
            for line in text.lines() {
                let mut f = line.splitn(4, '\t');
                let (Some(mark), Some(scroll), Some(url)) = (f.next(), f.next(), f.next()) else {
                    continue;
                };

                if url.is_empty() {
                    continue;
                }

                if mark == "*" {
                    tabs.active = tabs.list.len();
                }

                let id = tabs.next_id;
                tabs.next_id += 1;
                tabs.list.push(Tab {
                    id,
                    url: url.to_owned(),
                    title: f.next().unwrap_or("").to_owned(),
                    scroll: scroll.parse().unwrap_or(0),
                });
            }
        }

        tabs.first_load_is_new = !(restored && !tabs.list.is_empty());
        tabs
    }

    /// The saved active tab's address, to start on (see `load`).
    pub fn saved_active_url(path: Option<PathBuf>) -> Option<String> {
        let tabs = Self::load(path, true);

        tabs.list.get(tabs.active).map(|tab| tab.url.clone())
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn list(&self) -> &[Tab] {
        &self.list
    }

    pub fn active(&self) -> usize {
        self.active
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let text: String = self
            .list
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let mark = if i == self.active { "*" } else { "" };
                format!("{mark}\t{}\t{}\t{}\n", tab.scroll, clean(&tab.url), clean(&tab.title))
            })
            .collect();

        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }

        let tmp = path.with_extension("tsv.tmp");

        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }

    fn push_tab(&mut self, url: &str) -> usize {
        let id = self.next_id;

        self.next_id += 1;
        self.list.push(Tab { id, url: url.to_owned(), title: String::new(), scroll: 0 });
        self.list.len() - 1
    }

    /// The active tab now shows `url`. The first page of a run opens a new
    /// tab unless the run restored the saved one.
    pub fn on_page(&mut self, url: &str) {
        if self.first_load_is_new || self.list.is_empty() {
            self.first_load_is_new = false;

            let already = self.list.get(self.active).is_some_and(|tab| same(&tab.url, url));

            if !already {
                self.active = self.push_tab(url);
            }
        }

        if let Some(tab) = self.list.get_mut(self.active) {
            if tab.url != url {
                tab.url = url.to_owned();
                tab.title.clear();
            }
        }

        self.save();
    }

    pub fn on_title(&mut self, title: &str) {
        if let Some(tab) = self.list.get_mut(self.active) {
            if tab.title != title {
                tab.title = title.to_owned();
                self.save();
            }
        }
    }

    /// A tab for `url` after the others, left in the background.
    pub fn open_background(&mut self, url: &str) {
        self.push_tab(url);
        self.save();
    }

    pub fn set_scroll(&mut self, id: u64, scroll: i64) {
        if let Some(tab) = self.list.iter_mut().find(|tab| tab.id == id) {
            tab.scroll = scroll;
            self.save();
        }
    }

    /// The active tab's id, for its scroll report before leaving it.
    pub fn active_id(&self) -> Option<u64> {
        self.list.get(self.active).map(|tab| tab.id)
    }

    /// Make tab `index` active. Returns its address and scroll to load, or
    /// `None` when it already is active or does not exist.
    pub fn activate(&mut self, index: usize) -> Option<(String, i64)> {
        if index == self.active || index >= self.list.len() {
            return None;
        }

        self.active = index;
        self.save();

        let tab = &self.list[index];

        Some((tab.url.clone(), tab.scroll))
    }

    /// Close tab `index`. When it was the active one, the tab that takes
    /// its place (the next, else the previous) is returned to load. The
    /// last tab never closes.
    pub fn close(&mut self, index: usize) -> Option<(String, i64)> {
        if self.list.len() <= 1 || index >= self.list.len() {
            return None;
        }

        self.list.remove(index);

        let was_active = index == self.active;

        if index < self.active || self.active >= self.list.len() {
            self.active -= 1;
        }

        self.save();

        if was_active {
            let tab = &self.list[self.active];

            Some((tab.url.clone(), tab.scroll))
        } else {
            None
        }
    }

    /// A new tab for `url`, made active (the `[+]` button).
    pub fn open_active(&mut self, url: &str) {
        self.active = self.push_tab(url);
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tetron-browser-tabs-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("tabs.tsv")
    }

    fn urls(tabs: &Tabs) -> Vec<&str> {
        tabs.list().iter().map(|t| t.url.as_str()).collect()
    }

    #[test]
    fn first_page_makes_the_first_tab_and_navigation_updates_it() {
        let mut tabs = Tabs::new(None);

        tabs.on_page("https://a.org/");
        tabs.on_title("A");
        tabs.on_page("https://a.org/next");

        assert_eq!(urls(&tabs), ["https://a.org/next"]);
        assert_eq!(tabs.list()[0].title, "", "a new page drops the old title");
    }

    #[test]
    fn background_tabs_switching_and_closing() {
        let mut tabs = Tabs::new(None);

        tabs.on_page("https://a.org/");
        tabs.open_background("https://b.org/");
        tabs.open_background("https://c.org/");
        assert_eq!(tabs.active(), 0);

        let a = tabs.active_id().unwrap();
        tabs.set_scroll(a, 1200);
        assert_eq!(tabs.activate(2), Some(("https://c.org/".into(), 0)));
        assert_eq!(tabs.activate(2), None);
        assert_eq!(tabs.activate(0), Some(("https://a.org/".into(), 1200)));

        assert_eq!(tabs.close(1), None, "closing a background tab loads nothing");
        assert_eq!(urls(&tabs), ["https://a.org/", "https://c.org/"]);
        assert_eq!(tabs.close(0), Some(("https://c.org/".into(), 0)));
        assert_eq!(tabs.active(), 0);
        assert_eq!(tabs.close(0), None, "the last tab stays");

        tabs.open_background("https://d.org/");
        tabs.activate(1);
        assert_eq!(tabs.close(1), Some(("https://c.org/".into(), 0)), "last closes to the previous");
    }

    #[test]
    fn session_saves_and_restores() {
        let path = temp("session");
        let mut tabs = Tabs::new(Some(path.clone()));

        tabs.on_page("https://a.org/");
        tabs.on_title("A\ttab");
        tabs.open_background("https://b.org/");
        tabs.activate(1);
        let b = tabs.active_id().unwrap();
        tabs.set_scroll(b, 300);

        assert_eq!(Tabs::saved_active_url(Some(path.clone())).as_deref(), Some("https://b.org/"));

        // Started on the saved active tab: its first page is that tab.
        let mut restored = Tabs::load(Some(path.clone()), true);
        restored.on_page("https://b.org");
        assert_eq!(urls(&restored), ["https://a.org/", "https://b.org"]);
        assert_eq!(restored.active(), 1);
        assert_eq!(restored.list()[0].title, "A tab");
        assert_eq!(restored.list()[1].scroll, 300);

        // Started with a URL: it opens as a new tab after the saved ones.
        let mut with_url = Tabs::load(Some(path.clone()), false);
        with_url.on_page("https://new.org/");
        assert_eq!(with_url.len(), 3);
        assert_eq!(with_url.active(), 2);
    }
}

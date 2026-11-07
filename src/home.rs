//! The home page: what the browser opens when no URL is given, and what the
//! navigation bar's home button goes to.
//!
//! The first of these wins:
//! 1. `TETRON_BROWSER_HOME` (a URL or a file path),
//! 2. the first line of `<config>/tetron-browser/home` (same),
//! 3. `<config>/tetron-wm/bookmarks.html`, the bookmarks page shared with tetron-wm,
//! 4. `<config>/tetron-wm/start.html`, the page `tetron-wm-startpage` renders
//!    from its older `bookmarks.txt`,
//! 5. `<config>/tetron-browser/bookmarks.html`, written from the built-in
//!    default the first time it is needed.
//!
//! `<config>` is `$XDG_CONFIG_HOME`, else `~/.config`. The bookmarks pages are
//! Netscape bookmark files: valid HTML, and the format other browsers import
//! and export.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// The default bookmarks page, written to the user's config the first time.
pub const DEFAULT_BOOKMARKS: &str = include_str!("home/bookmarks.html");

/// Environment variable that overrides the home page.
pub const ENV_HOME: &str = "TETRON_BROWSER_HOME";

/// The home page URL for this user, writing the default bookmarks page if
/// nothing else is set up. `about:blank` when no config directory exists.
pub fn url() -> String {
    match config_dir() {
        Some(dir) => resolve(env::var(ENV_HOME).ok(), &dir),
        None => "about:blank".to_owned(),
    }
}

/// True when the arguments name a page to open: anything that is not an
/// option. Every option of tetron-browser and Chromium is a single
/// `-x`/`--name[=value]` argument.
pub fn has_url_arg(args: &[String]) -> bool {
    args.iter().any(|arg| !arg.starts_with('-'))
}

fn config_dir() -> Option<PathBuf> {
    match env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")),
    }
}

/// The home page given the override and the config directory (see the module
/// documentation for the order).
pub fn resolve(env_home: Option<String>, config: &Path) -> String {
    let own = config.join("tetron-browser");
    let configured = env_home.filter(|v| !v.trim().is_empty()).or_else(|| {
        fs::read_to_string(own.join("home")).ok().and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
        })
    });

    if let Some(home) = configured {
        return to_url(home.trim());
    }

    let wm = config.join("tetron-wm");

    for page in [wm.join("bookmarks.html"), wm.join("start.html")] {
        if page.is_file() {
            return file_url(&page);
        }
    }

    match own_bookmarks(config) {
        Some(page) => file_url(&page),
        None => "about:blank".to_owned(),
    }
}

/// The bookmarks file the bookmark button adds to: tetron-wm's when present
/// (one list for both), else tetron-browser's own, written from the default
/// the first time. `None` when it can not be written.
pub fn bookmarks_file() -> Option<PathBuf> {
    bookmarks_file_in(&config_dir()?)
}

pub fn bookmarks_file_in(config: &Path) -> Option<PathBuf> {
    let shared = config.join("tetron-wm/bookmarks.html");

    if shared.is_file() {
        Some(shared)
    } else {
        own_bookmarks(config)
    }
}

fn own_bookmarks(config: &Path) -> Option<PathBuf> {
    let own = config.join("tetron-browser");
    let page = own.join("bookmarks.html");

    if page.is_file()
        || (fs::create_dir_all(&own).is_ok() && fs::write(&page, DEFAULT_BOOKMARKS).is_ok())
    {
        Some(page)
    } else {
        None
    }
}

/// A URL as given, or a file path (absolute or `~/`) turned into a file URL.
fn to_url(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return file_url(&PathBuf::from(home).join(rest));
        }
    }

    if value.starts_with('/') {
        file_url(Path::new(value))
    } else {
        value.to_owned()
    }
}

fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");

    for char in path.to_string_lossy().chars() {
        match char {
            ' ' => url.push_str("%20"),
            '%' => url.push_str("%25"),
            '#' => url.push_str("%23"),
            '?' => url.push_str("%3F"),
            char => url.push(char),
        }
    }

    url
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tetron-browser-home-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn options_are_not_urls() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        assert!(!has_url_arg(&args(&[])));
        assert!(!has_url_arg(&args(&["--zoom=150", "-d", "--user-data-dir=/x y"])));
        assert!(has_url_arg(&args(&["--zoom=150", "example.com"])));
        assert!(has_url_arg(&args(&["https://example.com/?q=a"])));
    }

    #[test]
    fn writes_the_default_bookmarks_page_once_and_keeps_edits() {
        let dir = temp("default");
        let page = dir.join("tetron-browser/bookmarks.html");

        assert_eq!(resolve(None, &dir), format!("file://{}", page.display()));
        assert_eq!(fs::read_to_string(&page).unwrap(), DEFAULT_BOOKMARKS);
        assert!(DEFAULT_BOOKMARKS.starts_with("<!DOCTYPE NETSCAPE-Bookmark-file-1>"));

        fs::write(&page, "mine").unwrap();
        resolve(None, &dir);
        assert_eq!(fs::read_to_string(&page).unwrap(), "mine");
    }

    #[test]
    fn prefers_override_then_home_file_then_tetron_wm_pages() {
        let dir = temp("order");
        let wm = dir.join("tetron-wm");
        fs::create_dir_all(&wm).unwrap();

        fs::write(wm.join("start.html"), "").unwrap();
        assert_eq!(resolve(None, &dir), format!("file://{}/start.html", wm.display()));

        fs::write(wm.join("bookmarks.html"), "").unwrap();
        assert_eq!(resolve(None, &dir), format!("file://{}/bookmarks.html", wm.display()));

        fs::create_dir_all(dir.join("tetron-browser")).unwrap();
        fs::write(dir.join("tetron-browser/home"), "# my page\n\nhttps://example.com\n").unwrap();
        assert_eq!(resolve(None, &dir), "https://example.com");

        assert_eq!(resolve(Some("/srv/my page.html".into()), &dir), "file:///srv/my%20page.html");
        assert_eq!(resolve(Some("  ".into()), &dir), "https://example.com");
        assert!(!dir.join("tetron-browser/bookmarks.html").exists());

        assert_eq!(bookmarks_file_in(&dir), Some(wm.join("bookmarks.html")));
        fs::remove_file(wm.join("bookmarks.html")).unwrap();
        assert_eq!(bookmarks_file_in(&dir), Some(dir.join("tetron-browser/bookmarks.html")));
    }
}

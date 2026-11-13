//! Save link and Save image from the context menu: the file is fetched with
//! `curl` (else `wget`) into the download directory. The fetch does not carry
//! the page's cookies, so a file behind a login comes back as a web page;
//! that is reported instead of saved.
//!
//! Download directory, the first of: `TETRON_BROWSER_DOWNLOADS`, the first
//! line of `<config>/tetron-browser/downloads`, `~/Downloads`.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{ui::menu::percent_decode, utils::log};

pub const ENV_DOWNLOADS: &str = "TETRON_BROWSER_DOWNLOADS";

/// Sent with each fetch, so sites answer as they would the page's browser.
pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/111.0.0.0 Safari/537.36";

/// The download directory (see the module documentation), not created yet.
pub fn dir() -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let configured = env::var(ENV_DOWNLOADS).ok().filter(|v| !v.trim().is_empty()).or_else(|| {
        let file = crate::home::config_dir()?.join("tetron-browser").join("downloads");

        fs::read_to_string(file).ok()?.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned)
    });

    match configured {
        Some(path) => match path.trim().strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(path.trim()),
        },
        None => home.join("Downloads"),
    }
}

/// Saves `url` into `dir`. Returns the saved file or a message for USER.
pub fn save(url: &str, referer: Option<&str>, image: bool, dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("can not create {}: {e}", dir.display()))?;

    if let Some(rest) = url.strip_prefix("data:") {
        let (mime, bytes) = decode_data_url(rest).ok_or("can not read this data: URL")?;
        let path = unique(dir, &with_extension("image", &mime));

        fs::write(&path, bytes).map_err(|e| format!("can not write {}: {e}", path.display()))?;
        return Ok(path);
    }

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("can not save {} links yet", url.split(':').next().unwrap_or("these")));
    }

    let part = unique(dir, ".tetron-browser-download.part");
    let headers = fetch(url, referer, &part).map_err(|e| {
        let _ = fs::remove_file(&part);
        e
    })?;
    let header = |name: &str| last_header(&headers, name);
    let content_type = header("content-type").unwrap_or_default().to_ascii_lowercase();
    let name = header("content-disposition")
        .and_then(|value| disposition_name(&value))
        .or_else(|| url_name(url))
        .unwrap_or_else(|| "download".to_owned());
    let name = with_extension(&sanitize(&name), &content_type);

    if content_type.starts_with("text/html") && (image || !is_page_name(&url_name(url).unwrap_or_default())) {
        let _ = fs::remove_file(&part);
        return Err("not saved: the site sent a web page instead of the file (login needed?)".to_owned());
    }

    let path = unique(dir, &name);

    fs::rename(&part, &path).map_err(|e| {
        let _ = fs::remove_file(&part);
        format!("can not write {}: {e}", path.display())
    })?;
    Ok(path)
}

/// Schemes the browser shows itself; other links (`magnet:`, `mailto:`)
/// belong to another app.
const WEB_SCHEMES: [&str; 7] = ["http", "https", "file", "javascript", "data", "blob", "about"];

fn scheme(url: &str) -> String {
    url.split_once(':').map_or(String::new(), |(s, _)| s.to_ascii_lowercase())
}

/// A link for another app (`magnet:`, `mailto:`, ...): the browser can not show it.
pub fn is_external(url: &str) -> bool {
    let scheme = scheme(url);

    !scheme.is_empty() && !WEB_SCHEMES.contains(&scheme.as_str())
}

/// Whether "Open in default app" applies: links for another app, and links
/// to files (not web pages) over http, https or `file:`.
pub fn opens_in_app(url: &str) -> bool {
    match scheme(url).as_str() {
        "http" | "https" | "file" => url_name(url).is_some_and(|name| !is_page_name(&name)),
        _ => is_external(url),
    }
}

/// Hands `url` to the default app with `xdg-open`: a link for another app
/// as it is, a `file:` link as its path, an http(s) file after saving it
/// into `dir`. Returns a status line either way.
pub fn open_in_app(url: &str, referer: Option<&str>, dir: &Path) -> Result<String, String> {
    let target = match scheme(url).as_str() {
        "file" => percent_decode(url.trim_start_matches("file://").split(['?', '#']).next().unwrap_or("")),
        "http" | "https" => save(url, referer, false, dir)?.display().to_string(),
        _ if is_external(url) => url.to_owned(),
        _ => return Err(format!("can not open {url} in another app")),
    };
    let status = Command::new("xdg-open")
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "Open in default app needs xdg-open".to_owned())?;

    if status.success() {
        Ok(format!("opened {target}"))
    } else {
        log::warning!("xdg-open {target}: {status}");
        Err(format!("no default app opened {target} (xdg-open: {status})"))
    }
}

/// Fetches into `out`; returns the response headers of every hop.
fn fetch(url: &str, referer: Option<&str>, out: &Path) -> Result<String, String> {
    let referer = referer.filter(|r| r.starts_with("http"));
    let mut curl = Command::new("curl");

    curl.args(["--silent", "--show-error", "--fail", "--location", "--max-time", "600"])
        .args(["--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["--user-agent", USER_AGENT, "--dump-header", "-", "--output"])
        .arg(out);
    if let Some(referer) = referer {
        curl.args(["--referer", referer]);
    }
    curl.arg("--").arg(url);

    let output = match run(curl) {
        Some(output) => output,
        None => {
            let mut wget = Command::new("wget");

            wget.args(["--quiet", "--server-response", "--timeout=60", "--tries=2"])
                .args(["--user-agent", USER_AGENT, "-O"])
                .arg(out);
            if let Some(referer) = referer {
                wget.arg(format!("--referer={referer}"));
            }
            wget.arg("--").arg(url);
            run(wget).ok_or("saving needs curl or wget")?
        }
    };
    // curl writes the headers to stdout, wget to stderr.
    let headers = String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr);

    log::debug!("save {url}: {}", output.status);

    if output.status.success() {
        Ok(headers)
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        let line = error.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();

        log::warning!("save {url} failed: {} {line}", output.status);
        Err(format!("not saved: {}", if line.is_empty() { "the download failed" } else { line }))
    }
}

/// Runs a fetch command; `None` when the program is not installed.
fn run(mut command: Command) -> Option<std::process::Output> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output().ok()
}

/// The value of the last `name:` header line (the final hop after redirects).
fn last_header(headers: &str, name: &str) -> Option<String> {
    headers
        .lines()
        .filter_map(|line| line.trim().split_once(':'))
        .filter(|(key, _)| key.trim().eq_ignore_ascii_case(name))
        .last()
        .map(|(_, value)| value.trim().to_owned())
}

/// The file name in a Content-Disposition value (`filename*=` first).
fn disposition_name(value: &str) -> Option<String> {
    let params: Vec<(String, &str)> = value
        .split(';')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim()))
        .collect();
    let get = |key: &str| params.iter().find(|(k, _)| k == key).map(|(_, v)| *v);

    get("filename*")
        .and_then(|v| v.split_once("''").map(|(_, name)| percent_decode(name)))
        .or_else(|| get("filename").map(|v| v.trim_matches('"').to_owned()))
        .filter(|name| !name.trim().is_empty())
}

/// The last path segment of a URL, decoded.
fn url_name(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let path = path.split_once("://").map_or(path, |(_, rest)| rest.split_once('/').map_or("", |(_, p)| p));
    let name = percent_decode(path.rsplit('/').next()?);

    (!name.trim().is_empty()).then_some(name)
}

/// A link to a page (no extension or a page script): a web page is the file.
fn is_page_name(name: &str) -> bool {
    match name.rsplit_once('.') {
        None => true,
        Some((_, ext)) => ["html", "htm", "xhtml", "php", "asp", "aspx", "jsp", "cgi"].contains(&ext.to_ascii_lowercase().as_str()),
    }
}

/// A safe file name: no path separators or control characters, no leading
/// dots, at most 200 bytes.
fn sanitize(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    let mut clean = clean.trim().trim_start_matches('.').to_owned();

    while clean.len() > 200 {
        clean.pop();
    }

    if clean.is_empty() {
        "download".to_owned()
    } else {
        clean
    }
}

/// Adds an extension from the content type when the name has none.
fn with_extension(name: &str, content_type: &str) -> String {
    if name.contains('.') {
        return name.to_owned();
    }

    let mime = content_type.split(';').next().unwrap_or("").trim();
    let ext = match mime {
        "text/html" => "html",
        "text/plain" => "txt",
        "application/pdf" => "pdf",
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        _ => match mime.strip_prefix("image/") {
            Some(ext) if ext.chars().all(|c| c.is_ascii_alphanumeric()) && !ext.is_empty() => ext,
            _ => return name.to_owned(),
        },
    };

    format!("{name}.{ext}")
}

/// `name` in `dir`, or `name-1`, `name-2`, ... before the extension when it
/// exists: nothing is overwritten.
fn unique(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let mut path = dir.join(name);
    let mut n = 1;

    while path.exists() {
        path = dir.join(format!("{stem}-{n}{ext}"));
        n += 1;
    }

    path
}

/// The MIME type and bytes of a `data:` URL (without the `data:`).
fn decode_data_url(rest: &str) -> Option<(String, Vec<u8>)> {
    let (meta, payload) = rest.split_once(',')?;
    let base64 = meta.ends_with(";base64");
    let mime = meta.split(';').next().unwrap_or("").to_ascii_lowercase();
    let text = percent_decode(payload);
    let bytes = if base64 { decode_base64(&text)? } else { text.into_bytes() };

    Some((mime, bytes))
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);

    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };

        acc = acc << 6 | value as u32;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tetron-browser-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_come_from_the_header_then_the_url() {
        assert_eq!(disposition_name("attachment; filename=\"a b.pdf\"").as_deref(), Some("a b.pdf"));
        assert_eq!(disposition_name("attachment; filename=x.zip; filename*=UTF-8''%C3%A9t%C3%A9.zip").as_deref(), Some("été.zip"));
        assert_eq!(disposition_name("inline"), None);
        assert_eq!(url_name("https://a.org/files/r%20s.pdf?x=1#y").as_deref(), Some("r s.pdf"));
        assert_eq!(url_name("https://a.org/"), None);
        assert_eq!(url_name("https://a.org"), None);
        assert_eq!(last_header("HTTP/1.1 302\r\nContent-Type: text/html\r\n\r\nHTTP/1.1 200\r\ncontent-type: image/png\r\n", "Content-Type").as_deref(), Some("image/png"));
    }

    #[test]
    fn names_are_sanitized_and_get_extensions() {
        assert_eq!(sanitize("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize(".hidden"), "hidden");
        assert_eq!(sanitize("a\nb"), "a_b");
        assert_eq!(sanitize("..."), "download");
        assert_eq!(sanitize(&"x".repeat(300)).len(), 200);
        assert_eq!(with_extension("photo", "image/jpeg"), "photo.jpg");
        assert_eq!(with_extension("photo", "image/webp; q=1"), "photo.webp");
        assert_eq!(with_extension("a.png", "image/jpeg"), "a.png");
        assert_eq!(with_extension("blob", "application/octet-stream"), "blob");
        assert!(is_page_name("index") && is_page_name("x.PHP") && !is_page_name("r.pdf"));
    }

    #[test]
    fn default_app_applies_to_files_and_other_schemes_not_pages() {
        for url in ["magnet:?xt=urn:btih:abc", "mailto:a@b.org", "https://a.org/d.torrent", "file:///home/u/x.pdf", "HTTPS://a.org/r.ZIP"] {
            assert!(opens_in_app(url), "{url}");
        }
        for url in ["https://a.org/", "https://a.org/news", "https://a.org/i.html?x=1.pdf", "javascript:void(0)", "data:text/plain,x", "about:blank"] {
            assert!(!opens_in_app(url), "{url}");
        }
        assert!(is_external("magnet:?x") && !is_external("https://a.org/x.torrent") && !is_external("relative/x"));
        assert!(open_in_app("javascript:alert(1)", None, Path::new("/nonexistent")).is_err());
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let dir = scratch("unique");
        assert_eq!(unique(&dir, "a.pdf"), dir.join("a.pdf"));
        fs::write(dir.join("a.pdf"), "").unwrap();
        assert_eq!(unique(&dir, "a.pdf"), dir.join("a-1.pdf"));
        fs::write(dir.join("a-1.pdf"), "").unwrap();
        assert_eq!(unique(&dir, "a.pdf"), dir.join("a-2.pdf"));
        fs::write(dir.join("README"), "").unwrap();
        assert_eq!(unique(&dir, "README"), dir.join("README-1"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn data_urls_are_written_directly_and_odd_schemes_refused() {
        let dir = scratch("data");
        let path = save("data:image/png;base64,aGVsbG8=", None, true, &dir).unwrap();
        assert_eq!(path, dir.join("image.png"));
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        let path = save("data:text/plain,a%20b", None, true, &dir).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"a b");
        assert_eq!(save("blob:https://a.org/1", None, true, &dir).unwrap_err(), "can not save blob links yet");
        assert!(save("file:///etc/passwd", None, false, &dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}

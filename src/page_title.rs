//! Titles for tabs opened in the background: their page is not loaded until
//! the tab is shown, so only the start of the page is read for its `<title>`
//! (with curl for http and https, from disk for `file:` URLs).

use std::{
    fs::File,
    io::Read,
    process::{Command, Stdio},
};

use crate::ui::menu::percent_decode;

/// Bytes read at most: the title is in the head of the page.
const LIMIT: usize = 64 * 1024;

/// The page's title, `None` when it has none or can not be read.
pub fn fetch(url: &str) -> Option<String> {
    let head = if let Some(path) = url.strip_prefix("file://") {
        let path = percent_decode(path.split(['?', '#']).next()?);
        let mut head = Vec::new();

        File::open(path).ok()?.take(LIMIT as u64).read_to_end(&mut head).ok()?;
        head
    } else if url.starts_with("http://") || url.starts_with("https://") {
        read_curl(url)?
    } else {
        return None;
    };

    title(&String::from_utf8_lossy(&head))
}

/// The start of the page from curl, stopping after `</title>` or `LIMIT`.
fn read_curl(url: &str) -> Option<Vec<u8>> {
    let mut child = Command::new("curl")
        .args(["--silent", "--location", "--max-time", "10", "--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["--user-agent", crate::download::USER_AGENT, "--", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let mut head = Vec::new();
    let mut buffer = [0; 8192];

    while head.len() < LIMIT {
        match out.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buffer[..n]),
        }

        if head.windows(8).any(|w| w.eq_ignore_ascii_case(b"</title>")) {
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    Some(head)
}

/// The text of the first `<title>` element: entities decoded, white space
/// collapsed, at most 200 characters.
pub fn title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;

    // `<title>` or `<title attr>`, not `<titlebar>`.
    let open = loop {
        let at = from + lower[from..].find("<title")?;
        let next = lower[at + 6..].chars().next()?;

        if next == '>' || next.is_whitespace() {
            break at;
        }
        from = at + 6;
    };
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let text = decode_entities(&html[start..end]);
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: String = text.chars().take(200).collect();

    (!text.is_empty()).then_some(text)
}

/// The common HTML entities and numeric references.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];

        let decoded = rest[1..].find(';').filter(|&end| end <= 10).and_then(|end| {
            let name = &rest[1..1 + end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => match name.strip_prefix('#') {
                    Some(hex) if hex.starts_with(['x', 'X']) => u32::from_str_radix(&hex[1..], 16).ok().and_then(char::from_u32),
                    Some(dec) => dec.parse().ok().and_then(char::from_u32),
                    None => None,
                },
            };

            c.map(|c| (c, end + 2))
        });

        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }

    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_found_decoded_and_tidied() {
        assert_eq!(title("<html><head><TITLE>A &amp; B</TITLE>").as_deref(), Some("A & B"));
        assert_eq!(title("<title lang=en>\n  Two\n  lines </title>").as_deref(), Some("Two lines"));
        assert_eq!(title("<titlebar>x</titlebar><title>Real</title>").as_deref(), Some("Real"));
        assert_eq!(title("<title>&#169; &#x263A; &bogus; R&D</title>").as_deref(), Some("© ☺ &bogus; R&D"));
        assert_eq!(title("<title>   </title>"), None);
        assert_eq!(title("<title>no end"), None);
        assert_eq!(title("\u{89}PNG binary"), None);
        assert_eq!(title(&format!("<title>{}</title>", "x".repeat(300))).unwrap().len(), 200);
    }

    #[test]
    fn file_urls_are_read_from_disk() {
        let dir = std::env::temp_dir().join(format!("tetron-browser-title-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let page = dir.join("a page.html");
        std::fs::write(&page, "<html><title>On disk</title>").unwrap();
        let url = format!("file://{}", page.display()).replace(' ', "%20");
        assert_eq!(fetch(&url).as_deref(), Some("On disk"));
        assert_eq!(fetch("file:///nonexistent/x.html"), None);
        assert_eq!(fetch("about:blank"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

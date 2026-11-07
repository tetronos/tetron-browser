//! Adding and removing bookmarks in a Netscape bookmark file (the format of
//! the home page, see `home`). Folders are `<DT><H3>Name</H3>` followed by a
//! `<DL><p>` list; each bookmark is one `<DT><A HREF="...">Title</A>` line.
//! tetron-wm-startpage edits the same file the same way.

use std::{fs, io, path::Path};

/// Folder the bookmark button adds to (tetron-wm-startpage's default too).
pub const DEFAULT_FOLDER: &str = "Links";

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn href(url: &str) -> String {
    format!("HREF=\"{}\"", escape(url))
}

/// The forms of a URL that count as the same bookmark: with and without a
/// trailing `/` (Chromium shows `https://a.org` as `https://a.org/`).
fn needles(url: &str) -> [String; 2] {
    let bare = url.strip_suffix('/').unwrap_or(url);

    [href(bare).to_lowercase(), href(&format!("{bare}/")).to_lowercase()]
}

fn has_url(line: &str, url: &str) -> bool {
    let line = line.to_lowercase();

    needles(url).iter().any(|needle| line.contains(needle.as_str()))
}

/// True when the file has a bookmark for this URL (see `needles`).
pub fn contains(text: &str, url: &str) -> bool {
    text.lines().any(|line| has_url(line, url))
}

/// The file with a bookmark for `url` added at the top of `folder`, which is
/// created at the end when missing. Unchanged when the URL is already there.
pub fn add(text: &str, title: &str, url: &str, folder: &str, now: i64) -> String {
    if contains(text, url) {
        return text.to_owned();
    }

    let title = if title.trim().is_empty() { url } else { title.trim() };
    let entry = format!("<DT><A {} ADD_DATE=\"{now}\">{}</A>", href(url), escape(title));
    let heading = format!("<H3>{}</H3>", escape(folder)).to_lowercase();
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();

    // Into the folder: the line after its `<DL><p>`.
    if let Some(at) = lines.iter().position(|l| l.to_lowercase().contains(&heading)) {
        if let Some(open) = lines[at..].iter().position(|l| l.trim_start().to_uppercase().starts_with("<DL>")) {
            let indent = " ".repeat(lines[at].len() - lines[at].trim_start().len() + 4);
            lines.insert(at + open + 1, format!("{indent}{entry}"));
            return lines.join("\n") + "\n";
        }
    }

    // A new folder before the closing `</DL>` of the top list.
    let folder_lines = [
        format!("    <DT><H3>{}</H3>", escape(folder)),
        "    <DL><p>".to_owned(),
        format!("        {entry}"),
        "    </DL><p>".to_owned(),
    ];
    match lines.iter().rposition(|l| l.trim_start().to_uppercase().starts_with("</DL>")) {
        Some(close) => {
            lines.splice(close..close, folder_lines);
        }
        None => {
            lines.push("<DL><p>".to_owned());
            lines.extend(folder_lines);
            lines.push("</DL><p>".to_owned());
        }
    }

    lines.join("\n") + "\n"
}

/// The file without bookmark lines for `url`.
pub fn remove(text: &str, url: &str) -> String {
    text.lines()
        .filter(|line| !(has_url(line, url) && line.to_uppercase().contains("<A ")))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Add `url` to the file at `path`, or remove it when already there.
/// Returns whether it is bookmarked afterwards.
pub fn toggle(path: &Path, title: &str, url: &str) -> io::Result<bool> {
    let text = fs::read_to_string(path)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (next, marked) = if contains(&text, url) {
        (remove(&text, url), false)
    } else {
        (add(&text, title, url, DEFAULT_FOLDER, now), true)
    };
    // Write a sibling and rename it over, so a crash never leaves half a file.
    let tmp = path.with_extension("html.tmp");

    fs::write(&tmp, next)?;
    fs::rename(&tmp, path)?;

    Ok(marked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::DEFAULT_BOOKMARKS;

    #[test]
    fn adds_into_an_existing_folder_and_detects_it() {
        let text = add(DEFAULT_BOOKMARKS, "Example", "https://example.com/?a=1&b=2", "News", 7);
        let at = text.lines().position(|l| l.contains("<H3>News</H3>")).unwrap();

        assert_eq!(
            text.lines().nth(at + 2).unwrap(),
            "        <DT><A HREF=\"https://example.com/?a=1&amp;b=2\" ADD_DATE=\"7\">Example</A>"
        );
        assert!(contains(&text, "https://example.com/?a=1&b=2"));
        assert!(!contains(&text, "https://example.com/"));
        assert!(contains(DEFAULT_BOOKMARKS, "https://en.wikipedia.org/"));
        assert!(contains(DEFAULT_BOOKMARKS, "https://tetronos.github.io/tetron-wm"));
        assert!(!contains(DEFAULT_BOOKMARKS, "https://en.wikipedia.org/wiki"));
        assert_eq!(add(&text, "Again", "https://example.com/?a=1&b=2", "News", 8), text);
    }

    #[test]
    fn creates_the_folder_inside_the_top_list_and_removes_again() {
        let text = add(DEFAULT_BOOKMARKS, "<b>Title</b>", "https://x.org", DEFAULT_FOLDER, 1);
        let lines: Vec<&str> = text.lines().collect();
        let at = lines.iter().position(|l| l.contains("<H3>Links</H3>")).unwrap();

        assert_eq!(lines[at + 2].trim(), "<DT><A HREF=\"https://x.org\" ADD_DATE=\"1\">&lt;b&gt;Title&lt;/b&gt;</A>");
        assert_eq!(lines.last().unwrap().trim(), "</DL><p>");
        assert_eq!(lines[lines.len() - 2].trim(), "</DL><p>");

        let back = remove(&text, "https://x.org/");
        assert!(!contains(&back, "https://x.org"));
        assert!(contains(&back, "https://en.wikipedia.org"));
    }

    #[test]
    fn untitled_pages_use_the_url_and_bare_files_get_a_list() {
        let text = add("", " ", "https://y.org", DEFAULT_FOLDER, 1);

        assert!(text.contains(">https://y.org</A>"));
        assert!(text.starts_with("<DL><p>\n    <DT><H3>Links</H3>"));
    }

    #[test]
    fn toggle_writes_the_file_atomically() {
        let dir = std::env::temp_dir().join(format!("tetron-browser-bm-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bookmarks.html");
        fs::write(&path, DEFAULT_BOOKMARKS).unwrap();

        assert!(toggle(&path, "Z", "https://z.org").unwrap());
        assert!(contains(&fs::read_to_string(&path).unwrap(), "https://z.org"));
        assert!(!toggle(&path, "Z", "https://z.org").unwrap());
        assert!(!dir.join("bookmarks.html.tmp").exists());
    }
}

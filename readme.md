# tetron-browser

A web browser for the terminal. It renders pages with Chromium and draws them with truecolor blocks, with mouse and keyboard, in any terminal, over SSH, or on a bare console with no window server. It is the web browser of [TetronOS](https://github.com/tetronos/tetron-os) and of the [tetron-wm](https://github.com/tetronos/tetron-wm) terminal desktop.

The command is `tetron-browser`, or `twww` for short.

## Install

TetronOS, and tetron-wm's suite installer (essentials collection and up), install it for you. Otherwise, on Linux amd64:

- **Debian, Ubuntu and their derivatives, with the tetronos apt repository** (updated by `apt upgrade`): add the repository as described at https://tetronos.github.io/apt, then `sudo apt install tetron-browser`.
- **The release `.deb`**: download `tetron-browser_<version>-1_amd64.deb` from the [latest release](https://github.com/tetronos/tetron-browser/releases/latest), check it against its `.sha256`, then `sudo apt install ./tetron-browser_<version>-1_amd64.deb`.
- **Other distributions**: the release zip `tetron-browser.linux-amd64.zip` (with `.sha256`). Unpack it anywhere and run `tetron-browser` from that folder; it needs glibc 2.34 or newer and the usual Chromium libraries (nss, nspr, expat, alsa).

There is no arm64 or macOS build yet.

## Use

```sh
tetron-browser                     # the home page, or the tabs from last time
tetron-browser https://example.org
tetron-browser --help              # every option, key and file
```

Options: `-f/--fps` (default 60), `-z/--zoom` in percent, `-b/--bitmap` (text drawn as bitmaps), `-d/--debug`, `-v/--version`; most Chromium options work too.

What it does:

- **Navigation bar:** back, forward, reload, home `[⌂]`, bookmark `[☆]`, zoom `[−][100%][+]`. Click the address to edit it.
- **Home page:** your bookmarks with a search box. It is an ordinary Netscape bookmarks file, so you can edit it by hand or import it into another browser. tetron-wm's Start Page shows the same file.
- **Tabs:** middle-click a link, or use the right-click menu, to open it in a background tab; its title is read from the start of the page. Only the shown tab is a live page, so tabs cost no memory; switching loads the page again at its scroll position. Alt+1..9 switch, Alt+W closes. Tabs reopen on the next start.
- **Right-click menu** on links and images: open in a new tab, copy the address (terminal clipboard, OSC 52), save to the download folder, open in the default app (`xdg-open`). A left click on a `magnet:` or other non-web link opens it in the default app as well.

## Files

| What | Where |
|---|---|
| bookmarks and home page | `~/.config/tetron-browser/bookmarks.html`; another home page: `TETRON_BROWSER_HOME` or the file `~/.config/tetron-browser/home` |
| download folder | `TETRON_BROWSER_DOWNLOADS`, or the file `~/.config/tetron-browser/downloads`, default `~/Downloads` |
| browser profile (cookies, logins, cache) | `~/.config/tetron-browser/` (the package's `tetron-browser` command sets it; the zip's program uses Chromium's default unless you pass `--user-data-dir`) |
| saved tabs | `~/.local/state/tetron-browser/tabs.tsv` |

`~/.config` and `~/.local/state` follow `XDG_CONFIG_HOME` and `XDG_STATE_HOME` when set.

## Known limits

- **Old engine:** the Chromium inside is version 111 (early 2023) and has had no security fixes since. Use it for reading, and keep banking and other sensitive accounts in a maintained browser until the engine is updated (planned).
- **Cloudflare "verify you are human" checks** can not be passed: the click does not reach the checkbox.
- Files behind a login are not saved by the right-click menu (it downloads without the page's cookies).
- Dragging the sideways scroll bar's handle does not work yet; tilt the wheel, use Shift+wheel or the arrow keys.

## Build

tetron-browser has two parts: the Chromium program (the runtime), and `libcarbonyl.so`, a Rust library that draws the page in the terminal and handles input, built from this repository.

- **The Rust part:** `cargo build --release`, then put `target/release/libcarbonyl.so` next to the `tetron-browser` program of a release zip.
- **A release zip:** `scripts/release-tetron-browser.sh OUTDIR` (needs Docker) builds `libcarbonyl.so` on Debian 12 and packs it with the Chromium runtime, which is checked against a pinned SHA256.
- **Releases:** pushing a new version in `Cargo.toml` to main makes the GitHub workflow build the zip and the `.deb` and publish the release. Versions follow tetron core: 0.15.x until the TetronOS 1.0 release.
- **The Chromium runtime** is built from Chromium 111.0.5511.1 with the patches in `chromium/patches/` (`scripts/gclient.sh`, `scripts/patches.sh`, `scripts/gn.sh`, `scripts/build.sh`). That takes hours and about 100 GB of disk. Releases use a prebuilt runtime for now; building our own, and then moving to a current Chromium, is the next step.

Changes are listed in [changelog.md](changelog.md).

## Credits and license

tetron-browser continues [Carbonyl](https://github.com/fathyb/carbonyl) by Fathy Boundjadj, whose last release was v0.0.3 in February 2023; the Chromium runtime in releases is that release's, from an unmodified mirror ([`runtime-v0.0.3-upstream`](https://github.com/tetronos/tetron-browser/releases/tag/runtime-v0.0.3-upstream)). BSD-3-Clause, see [license.md](license.md); Chromium is under its own BSD-style license. Full notices: [packaging/copyright](packaging/copyright).

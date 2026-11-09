# Changelog

All notable changes to this project will be documented in this file.

## [0.15.4] - 2026-11-09

### Bug Fixes

- A click on a link's text opens the link at every zoom level: page text is squeezed into one terminal row, which drifts from where Chromium laid it out (more so when zoomed), so the click had to go a row or more above the link. A click on a cell that shows page text now goes to where Chromium painted that text; cells without text map as before

## [0.15.3] - 2026-11-07

### Changes

- Tabs: right-click or middle-click a link to open it in a background tab. With two or more tabs a strip shows under the navigation bar (click to switch, `×` to close, `[+]` for a new tab); Alt+1..9 switch and Alt+W closes. One page is live at a time: switching loads the tab's page again and restores its scroll position, so a background tab costs no memory. Tabs are saved in `~/.local/state/tetron-browser/tabs.tsv` and reopen on the next start without a URL; a URL opens as a new tab
- Alt+key reaches the browser as Alt+key (terminals send Escape then the key, which arrived as two separate presses)

### Bug Fixes

- Page text is drawn on the row where it is: it sat one row above its background and above where clicks land, so clicking a link on the row it showed on missed it
- A typed address was freed before Chromium read it (the string's memory was released when the input thread moved on); it now moves with the load request

## [0.15.2] - 2026-11-07

### Changes

- Opens the home page instead of a blank page when no URL is given: a bookmarks page with a DuckDuckGo Lite search box, in the Netscape bookmark file format other browsers import and export. It is `~/.config/tetron-browser/bookmarks.html`, created the first time from tetron-wm's older `bookmarks.txt` when that exists, else from a default list; tetron-wm's Start Page shows the same file. Works without tetron-wm. `TETRON_BROWSER_HOME` or `~/.config/tetron-browser/home` set another page (see `--help`)
- A home button `[⌂]` in the navigation bar, after reload
- A bookmark button `[☆]`: adds the page with its title to the "Links" folder of the bookmarks file; `[★]` shows a bookmarked page and removes it on a second click
- Zoom buttons `[−][100%][+]`: 50% to 300% in steps, the percentage resets to 100%. The zoom is a style on the page, so a new page starts at 100%
- Sideways scrolling with the mouse: a tilting wheel or sideways touchpad swipe, and Shift+wheel. Before, sideways wheel events and Shift+wheel scrolled up or down

## [0.15.1] - 2026-11-06

First published release of tetron-browser (the v0.15.0 tag was never released).

### Changes

- Releases are built by GitHub Actions: the linux-amd64 zip, its .sha256 and a .deb for Debian 12 and later (packaging/deb.toml, built with tetron-packaging)
- `twww` is a short command for `tetron-browser`
- The Chromium program is named tetron-browser, so ps and htop show the product name
- The `tetron-browser` command keeps the browser profile (cookies, logins, cache) in `~/.config/tetron-browser`; Chromium defaulted to the read-only install directory

### Bug Fixes

- Resizing no longer crashes the browser with SIGTRAP: Chromium's SIGWINCH handler resized the web contents inside the signal handler, on whichever thread got the signal (often the sandbox IPC thread, whose thread check stopped the process). The handler now only wakes a dispatcher, which runs the resize on the browser main thread once per burst

## [0.15.0] - 2026-11-06

### Changes

- Renamed to tetron-browser, a maintained continuation of Carbonyl: `--version` and `--help` say tetron-browser
- Versioned with the tetron suite: the minor version follows tetron core (0.15), based on Carbonyl 0.0.3

### Bug Fixes

- Clip draw rectangles to the terminal cell buffer: resizing to 1 or 2 columns panicked the render thread, and the next message to it aborted the browser with SIGTRAP
- A message to a dead render thread is dropped instead of panicking across the FFI boundary
- The render thread exits when its channel disconnects instead of looping; the frame deadline saturates; dropping the renderer stops the thread

## [0.0.3] - 2023-02-18

### 🚀 Features

- Add `--help` and `--version` ([#105](https://github.com/fathyb/carbonyl/issues/105))
- Add logo and description to `--help` ([#106](https://github.com/fathyb/carbonyl/issues/106))
- Use Cmd instead of Alt for navigation shortcuts ([#109](https://github.com/fathyb/carbonyl/issues/109))
- Enable h.264 support ([#103](https://github.com/fathyb/carbonyl/issues/103))
- Introduce quadrant rendering ([#120](https://github.com/fathyb/carbonyl/issues/120))

### 🐛 Bug Fixes

- Fix arguments parsing ([#108](https://github.com/fathyb/carbonyl/issues/108))
- Fix missing module error on npm package ([#113](https://github.com/fathyb/carbonyl/issues/113))
- Enable threaded compositing with bitmap mode
- Fix idling CPU usage ([#126](https://github.com/fathyb/carbonyl/issues/126))
- Package proper library in binaries ([#127](https://github.com/fathyb/carbonyl/issues/127))

### 📖 Documentation

- Update download links
- Fix commit_preprocessors url ([#102](https://github.com/fathyb/carbonyl/issues/102))
- Add `--rm` to Docker example ([#101](https://github.com/fathyb/carbonyl/issues/101))

## [0.0.2] - 2023-02-09

### 🚀 Features

- Better true color detection
- Linux support
- Xterm title
- Hide stderr unless crash
- Add `--debug` to print stderr on exit ([#23](https://github.com/fathyb/carbonyl/issues/23))
- Add navigation UI ([#86](https://github.com/fathyb/carbonyl/issues/86))
- Handle terminal resize ([#87](https://github.com/fathyb/carbonyl/issues/87))

### 🐛 Bug Fixes

- Parser fixes
- Properly enter tab and return keys
- Fix some special characters ([#35](https://github.com/fathyb/carbonyl/issues/35))
- Improve terminal size detection ([#36](https://github.com/fathyb/carbonyl/issues/36))
- Allow working directories that contain spaces ([#63](https://github.com/fathyb/carbonyl/issues/63))
- Do not use tags for checkout ([#64](https://github.com/fathyb/carbonyl/issues/64))
- Do not checkout nacl ([#79](https://github.com/fathyb/carbonyl/issues/79))
- Wrap zip files in carbonyl folder ([#88](https://github.com/fathyb/carbonyl/issues/88))
- Fix WebGL support on Linux ([#90](https://github.com/fathyb/carbonyl/issues/90))
- Fix initial freeze on Docker ([#91](https://github.com/fathyb/carbonyl/issues/91))

### 📖 Documentation

- Upload demo videos
- Fix video layout
- Fix a typo ([#1](https://github.com/fathyb/carbonyl/issues/1))
- Fix a typo `ie.` -> `i.e.` ([#9](https://github.com/fathyb/carbonyl/issues/9))
- Fix build instructions ([#15](https://github.com/fathyb/carbonyl/issues/15))
- Add ascii logo
- Add comparisons ([#34](https://github.com/fathyb/carbonyl/issues/34))
- Add OS support ([#50](https://github.com/fathyb/carbonyl/issues/50))
- Add download link
- Fix linux download links
- Document shared library
- Fix a typo (`know` -> `known`) ([#71](https://github.com/fathyb/carbonyl/issues/71))
- Add license

### Build

- Various build system fixes ([#20](https://github.com/fathyb/carbonyl/issues/20))


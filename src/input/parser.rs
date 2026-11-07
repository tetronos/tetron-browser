use std::ops::ControlFlow;

use crate::input::*;

#[derive(Default)]
pub struct Parser {
    events: Vec<Event>,
    sequence: Sequence,
}

#[derive(Default)]
enum Sequence {
    #[default]
    Char,
    Escape,
    Control,
    Mouse(Mouse),
    Keyboard(Keyboard),
    DeviceControl(DeviceControl),
}

#[derive(Clone, Debug)]
pub enum TerminalEvent {
    Name(String),
    TrueColorSupported,
}

#[derive(Clone, Debug)]
pub enum Event {
    KeyPress { key: Key },
    /// `button`: 0 left, 1 middle, 2 right.
    MouseUp { row: usize, col: usize, button: u8 },
    MouseDown { row: usize, col: usize, button: u8 },
    MouseMove { row: usize, col: usize },
    Scroll { delta: isize },
    /// Sideways wheel or Shift+wheel, in notches: positive is right.
    ScrollX { delta: isize },
    Terminal(TerminalEvent),
    Exit,
}

pub type ParseControlFlow = ControlFlow<Option<Event>, Option<Event>>;

impl Parser {
    pub fn new() -> Parser {
        Self::default()
    }

    pub fn parse(&mut self, input: &[u8]) -> Vec<Event> {
        let mut sequence = std::mem::take(&mut self.sequence);

        macro_rules! emit {
            ($event:expr) => {{
                if let Some(event) = $event.into() {
                    self.events.push(event);
                }

                Sequence::Char
            }};
            ($event:expr; continue) => {{
                if let Some(event) = $event.into() {
                    self.events.push(event);
                }

                continue;
            }};
        }
        macro_rules! parse {
            ($parser:expr, $key:expr) => (
                match $parser.parse($key) {
                    ControlFlow::Break(None) => Sequence::Char,
                    ControlFlow::Break(Some(event)) => emit!(event),
                    ControlFlow::Continue(None) => continue,
                    ControlFlow::Continue(Some(event)) => emit!(event; continue),
                }
            );
        }

        for &key in input {
            sequence = match sequence {
                Sequence::Char => match key {
                    0x1b => Sequence::Escape,
                    0x03 => emit!(Event::Exit),
                    key => emit!(Event::KeyPress { key: key.into() }),
                },
                Sequence::Escape => match key {
                    b'[' => Sequence::Control,
                    b'P' => Sequence::DeviceControl(DeviceControl::new()),
                    0x1b => emit!(Event::KeyPress { key: 0x1b.into() }; continue),
                    // Alt+key: terminals send Escape then the key. `O` is
                    // left alone: it starts the SS3 keys (arrows in some modes).
                    key @ 0x20..=0x7e if key != b'O' => emit!(Event::KeyPress {
                        key: Key {
                            char: key,
                            modifiers: KeyModifiers { alt: true, ..Default::default() },
                        },
                    }),
                    key => {
                        emit!(Event::KeyPress { key: 0x1b.into() });
                        emit!(Event::KeyPress { key: key.into() })
                    }
                },
                Sequence::Control => match key {
                    b'<' => Sequence::Mouse(Mouse::new()),
                    b'1' => Sequence::Keyboard(Keyboard::new()),
                    key => emit!(Keyboard::key(key, 0)),
                },
                Sequence::Mouse(ref mut mouse) => parse!(mouse, key),
                Sequence::Keyboard(ref mut keyboard) => parse!(keyboard, key),
                Sequence::DeviceControl(ref mut dcs) => parse!(dcs, key),
            }
        }

        self.sequence = sequence;

        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod alt_tests {
    use super::*;

    fn keys(input: &[u8]) -> Vec<(u8, bool)> {
        Parser::new()
            .parse(input)
            .into_iter()
            .filter_map(|event| match event {
                Event::KeyPress { key } => Some((key.char, key.modifiers.alt)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn escape_then_a_key_is_alt_key() {
        assert_eq!(keys(b"\x1b1"), [(b'1', true)]);
        assert_eq!(keys(b"\x1bw"), [(b'w', true)]);
        assert_eq!(keys(b"w"), [(b'w', false)]);
        assert_eq!(keys(b"\x1bO"), [(0x1b, false), (b'O', false)], "SS3 start stays as before");
        assert_eq!(keys(b"\x1b[1;3D"), [(0x14, true)], "Alt+Left still parses");
    }
}

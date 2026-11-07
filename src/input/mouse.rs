use std::ops::BitAnd;

use crate::{control_flow, utils::log};

use super::{Event, ParseControlFlow};

#[derive(Default, Clone, Debug)]
pub struct Mouse {
    buf: Vec<u8>,
    btn: Option<u32>,
    col: Option<u32>,
    row: Option<u32>,
}

impl Mouse {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(&mut self, key: u8) -> ParseControlFlow {
        match key {
            b'm' | b'M' => control_flow!(break self.get(key)),
            b';' => match self.read() {
                None => control_flow!(break),
                Some(()) => control_flow!(continue),
            },
            key => control_flow!(self.buf.push(key); continue),
        }
    }

    fn read(&mut self) -> Option<()> {
        let buf = std::mem::take(&mut self.buf);
        let str = std::str::from_utf8(&buf).ok()?;
        let num = Some(str.parse().ok()?);

        match (self.btn, self.col, self.row) {
            (None, _, _) => self.btn = num,
            (_, None, _) => self.col = num,
            (_, _, None) => self.row = num,
            _ => {
                log::warning!("Malformed mouse sequence");

                return None;
            }
        }

        return Some(());
    }

    fn get(&mut self, key: u8) -> Option<Event> {
        let (btn, col, row) = {
            self.read()?;

            (self.btn?, self.col?, self.row?)
        };

        Some({
            if let Some(event) = wheel(btn) {
                event
            } else {
                let col = col as usize - 1;
                let row = row as usize - 1;
                let button = (btn & 0x03) as u8;

                if key == b'm' {
                    Event::MouseUp { row, col, button }
                } else if Mask::MouseMove & btn {
                    Event::MouseMove { row, col }
                } else {
                    Event::MouseDown { row, col, button }
                }
            }
        })
    }
}

/// A wheel event from an SGR mouse button code: bit 0x40 marks the wheel,
/// the low two bits say up, down, left or right, and 0x04 is Shift (Shift
/// turns up/down into left/right, as in most programs). Motion (0x20) and
/// other modifier bits are ignored.
fn wheel(btn: u32) -> Option<Event> {
    if btn & 0x40 == 0 || btn & 0x80 != 0 {
        return None;
    }

    let shift = btn & 0x04 != 0;

    Some(match (btn & 0x03, shift) {
        (0, false) => Event::Scroll { delta: 1 },
        (1, false) => Event::Scroll { delta: -1 },
        (0, true) | (2, _) => Event::ScrollX { delta: -1 },
        (1, true) | (3, _) => Event::ScrollX { delta: 1 },
        _ => unreachable!(),
    })
}

enum Mask {
    MouseMove = 0x20,
}

impl BitAnd<u32> for Mask {
    type Output = bool;

    fn bitand(self, rhs: u32) -> bool {
        let mask = self as u32;

        mask & rhs == mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::ControlFlow;

    fn event(seq: &str) -> Option<Event> {
        let mut mouse = Mouse::new();

        for &byte in seq.as_bytes() {
            if let ControlFlow::Break(event) = mouse.parse(byte) {
                return event;
            }
        }

        None
    }

    fn delta(seq: &str) -> (isize, isize) {
        match event(seq) {
            Some(Event::Scroll { delta }) => (0, delta),
            Some(Event::ScrollX { delta }) => (delta, 0),
            other => panic!("{seq}: {other:?}"),
        }
    }

    #[test]
    fn wheel_directions_and_shift() {
        assert_eq!(delta("64;5;5M"), (0, 1));
        assert_eq!(delta("65;5;5M"), (0, -1));
        assert_eq!(delta("66;5;5M"), (-1, 0));
        assert_eq!(delta("67;5;5M"), (1, 0));
        assert_eq!(delta("68;5;5M"), (-1, 0));
        assert_eq!(delta("69;5;5M"), (1, 0));
        assert!(matches!(event("0;5;5M"), Some(Event::MouseDown { col: 4, row: 4, button: 0 })));
        assert!(matches!(event("2;5;5M"), Some(Event::MouseDown { button: 2, .. })));
        assert!(matches!(event("1;5;5m"), Some(Event::MouseUp { button: 1, .. })));
    }
}

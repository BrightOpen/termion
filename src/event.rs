//! Mouse and key events.

use std::io::{Error, ErrorKind};
use std::str;

/// An event reported by the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Event {
    /// A key press.
    Key(Key),
    /// A mouse button press, release or wheel use at specific coordinates.
    Mouse(MouseEvent),
    /// An event that cannot currently be evaluated.
    Unsupported(Vec<u8>),
}

/// A mouse related event.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum MouseEvent {
    /// A mouse button was pressed.
    ///
    /// The coordinates are one-based.
    Press(MouseButton, u16, u16),
    /// A mouse button was released.
    ///
    /// The coordinates are one-based.
    Release(u16, u16),
    /// A mouse button is held over the given coordinates.
    ///
    /// The coordinates are one-based.
    Hold(u16, u16),
}

/// A mouse button.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// The left mouse button.
    Left,
    /// The right mouse button.
    Right,
    /// The middle mouse button.
    Middle,
    /// Mouse wheel is going up.
    ///
    /// This event is typically only used with Mouse::Press.
    WheelUp,
    /// Mouse wheel is going down.
    ///
    /// This event is typically only used with Mouse::Press.
    WheelDown,
}

/// A key.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// Backspace.
    Backspace,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page Up key.
    PageUp,
    /// Page Down key.
    PageDown,
    /// Delete key.
    Delete,
    /// Insert key.
    Insert,
    /// Function keys.
    ///
    /// Only function keys 1 through 12 are supported.
    F(u8),
    /// Normal character.
    Char(char),
    /// Alt modified character.
    Alt(char),
    /// Ctrl modified character.
    ///
    /// Note that certain keys may not be modifiable with `ctrl`, due to limitations of terminals.
    Ctrl(char),
    /// Null byte.
    Null,
    /// Esc key.
    Esc,

    #[doc(hidden)]
    __IsNotComplete,
}

pub fn parse_event<I>(item: u8, iter: &mut I) -> Result<(Event, Vec<u8>), Error>
where
    I: Iterator<Item = Result<u8, Error>>,
{
    let mut buf = vec![item];
    let result = {
        let mut iter = iter.inspect(|byte| {
            if let &Ok(byte) = byte {
                buf.push(byte);
            }
        });
        try_parse_event(item, &mut iter)
    };
    result
        .or_else(|err| {
            warn!("Event parse error: {}", err);
            Ok(Event::Unsupported(buf.clone()))
        })
        .map(|e| {
            debug!("Event: {:?}", e);
            (e, buf)
        })
}

/// Parse an Event from `item` and possibly subsequent bytes through `iter`.
fn try_parse_event<I>(item: u8, iter: &mut I) -> Result<Event, Error>
where
    I: Iterator<Item = Result<u8, Error>>,
{
    match item {
        b'\x1B' => {
            // This is an escape character, leading a control sequence.
            Ok(match iter.next() {
                Some(Ok(b'O')) => {
                    match iter.next() {
                        // F1-F4
                        Some(Ok(val @ b'P'...b'S')) => Event::Key(Key::F(1 + val - b'P')),
                        Some(Ok(val)) => Event::Unsupported(vec![b'\x1B', b'0', val]),
                        Some(Err(e)) => return Err(e),
                        None => Event::Unsupported(vec![b'\x1B', b'0']),
                    }
                }
                Some(Ok(b'[')) => {
                    // This is a CSI sequence.
                    parse_csi(iter)?
                }
                Some(Ok(c)) => {
                    let ch = parse_utf8_char(c, iter);
                    Event::Key(Key::Alt(try!(ch)))
                }
                Some(Err(e)) => return Err(e),
                None => Event::Unsupported(vec![b'\x1B']),
            })
        }
        b'\n' | b'\r' => Ok(Event::Key(Key::Char('\n'))),
        b'\t' => Ok(Event::Key(Key::Char('\t'))),
        b'\x7F' => Ok(Event::Key(Key::Backspace)),
        c @ b'\x01'...b'\x1A' => Ok(Event::Key(Key::Ctrl((c as u8 - 0x1 + b'a') as char))),
        c @ b'\x1C'...b'\x1F' => Ok(Event::Key(Key::Ctrl((c as u8 - 0x1C + b'4') as char))),
        b'\0' => Ok(Event::Key(Key::Null)),
        c => Ok({
            let ch = parse_utf8_char(c, iter);
            Event::Key(Key::Char(try!(ch)))
        }),
    }
}

fn pop<I, T>(iter: &mut I) -> Result<T, Error>
where
    I: Iterator<Item = Result<T, Error>>,
{
    iter.next().unwrap_or(Err(err_unexpected_eof()))
}

fn err_invalid_input() -> Error {
    Error::from(ErrorKind::InvalidInput)
}
fn err_unexpected_eof() -> Error {
    Error::from(ErrorKind::UnexpectedEof)
}

/// Parses a CSI sequence, just after reading ^[
///
/// Returns Ok(Event::Unsupported) if an unrecognized sequence is found.
fn parse_csi<I>(iter: &mut I) -> Result<Event, Error>
where
    I: Iterator<Item = Result<u8, Error>>,
{
    Ok(match pop(iter)? {
        b'[' => match iter.next() {
            None => return Err(err_unexpected_eof()),
            Some(Ok(val @ b'A'...b'E')) => Event::Key(Key::F(1 + val - b'A')),
            Some(Ok(_)) => return Err(err_invalid_input()),
            Some(Err(e)) => return Err(e),
        },
        b'D' => Event::Key(Key::Left),
        b'C' => Event::Key(Key::Right),
        b'A' => Event::Key(Key::Up),
        b'B' => Event::Key(Key::Down),
        b'H' => Event::Key(Key::Home),
        b'F' => Event::Key(Key::End),
        b'M' => {
            // X10 emulation mouse encoding: ESC [ CB Cx Cy (6 characters only).

            let b1 = pop(iter)?;
            let b2 = pop(iter)?;
            let b3 = pop(iter)?;

            let cb = b1 as i8 - 32;
            // (1, 1) are the coords for upper left.
            let cx = b2.saturating_sub(32) as u16;
            let cy = b3.saturating_sub(32) as u16;
            Event::Mouse(match cb & 0b11 {
                0 => {
                    if cb & 0x40 != 0 {
                        MouseEvent::Press(MouseButton::WheelUp, cx, cy)
                    } else {
                        MouseEvent::Press(MouseButton::Left, cx, cy)
                    }
                }
                1 => {
                    if cb & 0x40 != 0 {
                        MouseEvent::Press(MouseButton::WheelDown, cx, cy)
                    } else {
                        MouseEvent::Press(MouseButton::Middle, cx, cy)
                    }
                }
                2 => MouseEvent::Press(MouseButton::Right, cx, cy),
                3 => MouseEvent::Release(cx, cy),
                _ => return Err(err_invalid_input()),
            })
        }
        b'<' => {
            // xterm mouse encoding:
            // ESC [ < Cb ; Cx ; Cy (;) (M or m)
            let mut buf = Vec::new();
            let mut c = pop(iter)?;
            while match c {
                b'm' | b'M' => false,
                _ => true,
            } {
                buf.push(c);
                c = pop(iter)?;
            }
            let str_buf = String::from_utf8(buf).map_err(|_| err_invalid_input())?;
            let nums = &mut str_buf
                .split(';')
                .map(|n| n.parse::<u16>().map_err(|_| err_invalid_input()));

            let cb = pop(nums)?;
            let cx = pop(nums)?;
            let cy = pop(nums)?;

            let event = match cb {
                0...2 | 64...65 => {
                    let button = match cb {
                        0 => MouseButton::Left,
                        1 => MouseButton::Middle,
                        2 => MouseButton::Right,
                        64 => MouseButton::WheelUp,
                        65 => MouseButton::WheelDown,
                        _ => unreachable!(),
                    };
                    match c {
                        b'M' => MouseEvent::Press(button, cx, cy),
                        b'm' => MouseEvent::Release(cx, cy),
                        _ => return Err(err_invalid_input()),
                    }
                }
                32 => MouseEvent::Hold(cx, cy),
                3 => MouseEvent::Release(cx, cy),
                _ => return Err(err_invalid_input()),
            };

            Event::Mouse(event)
        }
        mut c @ b'0'...b'9' => {
            // Numbered escape code.
            let mut buf = Vec::new();
            buf.push(c);
            // The final byte of a CSI sequence can be in the range 64-126, so
            // let's keep reading anything else.
            while let Some(n) = iter.next() {
                c = n?;
                if c < 64 || c > 126 {
                    buf.push(c);
                } else {
                    break;
                }
            }

            match c {
                // rxvt mouse encoding:
                // ESC [ Cb ; Cx ; Cy ; M
                b'M' => {
                    let str_buf = String::from_utf8(buf).map_err(|_| err_invalid_input())?;

                    let mut nums = str_buf
                        .split(';')
                        .map(|n| n.parse().map_err(|_| err_invalid_input()));

                    let cb = pop(&mut nums)?;
                    let cx = pop(&mut nums)?;
                    let cy = pop(&mut nums)?;

                    let event = match cb {
                        32 => MouseEvent::Press(MouseButton::Left, cx, cy),
                        33 => MouseEvent::Press(MouseButton::Middle, cx, cy),
                        34 => MouseEvent::Press(MouseButton::Right, cx, cy),
                        35 => MouseEvent::Release(cx, cy),
                        64 => MouseEvent::Hold(cx, cy),
                        96 | 97 => MouseEvent::Press(MouseButton::WheelUp, cx, cy),
                        _ => {
                            return Err(err_invalid_input());
                        }
                    };

                    Event::Mouse(event)
                }
                // Special key code.
                b'~' => {
                    let str_buf = String::from_utf8(buf).map_err(|_| err_invalid_input())?;

                    // This CSI sequence can be a list of semicolon-separated
                    // numbers.
                    let mut nums = str_buf
                        .split(';')
                        .map(|n| n.parse().map_err(|_| err_invalid_input()));

                    let num = pop(&mut nums)?;

                    // TODO: handle multiple values for key modififiers (ex: values
                    // [3, 2] means Shift+Delete)
                    if let Some(_) = nums.next() {
                        return Err(err_invalid_input());
                    }

                    match num {
                        1 | 7 => Event::Key(Key::Home),
                        2 => Event::Key(Key::Insert),
                        3 => Event::Key(Key::Delete),
                        4 | 8 => Event::Key(Key::End),
                        5 => Event::Key(Key::PageUp),
                        6 => Event::Key(Key::PageDown),
                        v @ 11...15 => Event::Key(Key::F(v - 10)),
                        v @ 17...21 => Event::Key(Key::F(v - 11)),
                        v @ 23...24 => Event::Key(Key::F(v - 12)),
                        _ => return Err(err_invalid_input()),
                    }
                }
                _ => return Err(err_invalid_input()),
            }
        }
        _ => return Err(err_invalid_input()),
    })
}

/// Parse `c` as either a single byte ASCII char or a variable size UTF-8 char.
fn parse_utf8_char<I>(c: u8, iter: &mut I) -> Result<char, Error>
where
    I: Iterator<Item = Result<u8, Error>>,
{
    let error = Err(Error::new(
        ErrorKind::Other,
        "Input character is not valid UTF-8",
    ));
    if c.is_ascii() {
        Ok(c as char)
    } else {
        let bytes = &mut Vec::new();
        bytes.push(c);

        loop {
            match iter.next() {
                Some(Ok(next)) => {
                    bytes.push(next);
                    if let Ok(st) = str::from_utf8(bytes) {
                        // unwrap is safe here because parse was OK
                        return Ok(st.chars().next().unwrap());
                    }
                    if bytes.len() >= 4 {
                        return error;
                    }
                }
                _ => return error,
            }
        }
    }
}

#[cfg(test)]
#[test]
fn test_parse_utf8() {
    let st = "abcéŷ¤£€ù%323";
    let ref mut bytes = st.bytes().map(|x| Ok(x));
    let chars = st.chars();
    for c in chars {
        let b = bytes.next().unwrap().unwrap();
        assert!(c == parse_utf8_char(b, bytes).unwrap());
    }
}

#[test]
fn test_parse_invalid_mouse() {
    let item = b'\x1B';
    let mut iter = "[x".bytes().map(|x| Ok(x));
    assert_eq!(
        parse_event(item, &mut iter).unwrap(),
        (
            Event::Unsupported(vec![b'\x1B', b'[', b'x']),
            vec![b'\x1B', b'[', b'x']
        )
    )
}

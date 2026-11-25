//! Where text being parsed is, however often it is read.
//!
//! An address is no such place: a command that is rewritten and re-parsed
//! (`\pu{…}`, `\Braket{…}`) is in a buffer of its own, which a speculative
//! parse frees and makes again, at the same address or another.

use super::*;

/// A place in the text being parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Place {
    /// 0 is the whole text; see [`Buffers`].
    buffer: u32,
    offset: usize,
}

/// The texts being parsed, one in another.
#[derive(Default)]
pub(super) struct Buffers {
    /// Address, length and number of each, the innermost last.
    open: SmallVec<[(usize, usize, u32); 2]>,
    /// The numbers of the buffers, by the places of the commands they are the
    /// rewritten text of: the same when a command is read again.
    numbers: HashMap<Place, u32>,
}

impl LatexCompactParser {
    /// Start reading the whole text.
    pub(super) fn open_text(&mut self, text: &str) {
        self.buffers.open.clear();
        self.buffers
            .open
            .push((text.as_ptr() as usize, text.len(), 0));
    }

    /// Start reading `text`, which the command at `origin` is rewritten to.
    pub(super) fn open_buffer(&mut self, origin: &str, text: &str) {
        let place = self.place(origin);
        let next = u32::try_from(self.buffers.numbers.len() + 1).unwrap_or(u32::MAX);
        let number = *self.buffers.numbers.entry(place).or_insert(next);
        self.buffers
            .open
            .push((text.as_ptr() as usize, text.len(), number));
    }

    /// Stop reading the innermost text.
    pub(super) fn close_buffer(&mut self) {
        self.buffers.open.pop();
    }

    /// The place `text`, of a text that is being parsed, starts at. The end of
    /// a text is a place in it. It may be the address of the start of another:
    /// what is read is the innermost, which is looked at first.
    pub(super) fn place(&self, text: &str) -> Place {
        let address = text.as_ptr() as usize;
        self.buffers
            .open
            .iter()
            .rev()
            .find_map(|&(start, len, buffer)| {
                let offset = address.checked_sub(start).filter(|&offset| offset <= len)?;
                Some(Place { buffer, offset })
            })
            .unwrap_or_else(|| {
                // (A parser that was given no whole text reads only that.)
                debug_assert!(
                    self.buffers.open.is_empty(),
                    "text that is parsed is in no buffer: {text:?}"
                );
                Place {
                    buffer: u32::MAX,
                    offset: 0,
                }
            })
    }
}

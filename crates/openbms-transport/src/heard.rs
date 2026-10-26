//! What a passive listen on a bus heard, collected in one place for every listener (`Connection`'s first listen under
//! the bus rule, `openbms_flash::maint::bus::listen`): the bytes, and the addresses a Seplos master polled (CID2 0x5A
//! with an empty INFO), recognised line by line across reads.

use std::collections::BTreeSet;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Heard {
    /// bytes received
    pub bytes: usize,
    /// addresses the master polled
    pub polled: BTreeSet<u8>,
    line: Vec<u8>,
}

impl Heard {
    /// Take the next bytes read.
    pub fn feed(&mut self, data: &[u8]) {
        self.bytes += data.len();
        for &b in data {
            self.line.push(b);
            if b == b'\r' {
                let start = self.line.iter().position(|c| *c == b'~').unwrap_or(self.line.len());
                if let Ok(f) = openbms_proto::frame::parse(&self.line[start..]) {
                    if f.rtn == 0x5A && f.info.is_empty() {
                        self.polled.insert(f.address);
                    }
                }
                self.line.clear();
            }
        }
        // garbage without a line end is no frame
        if self.line.len() > 4096 {
            self.line.clear();
        }
    }

    pub fn quiet(&self) -> bool {
        self.bytes == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_lines_are_recognised_across_reads() {
        let mut h = Heard::default();
        h.feed(b"~2001465A00");
        h.feed(b"00FD9D\r~2002465A0000FD9C\r");
        assert_eq!(h.polled.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
        assert!(!h.quiet());
        let mut noise = Heard::default();
        noise.feed(&[0xF0, 0x0F, b'\r']);
        assert!(noise.polled.is_empty() && !noise.quiet());
        assert!(Heard::default().quiet());
    }
}

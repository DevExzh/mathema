//! Generated MTEF without size records: no node has a size. A structure gives
//! none, however characters, embellishments, scripts and templates nest.

use mathema::{
    Config, MtefVersion, MtefWriteOptions, Strictness, from_mtef_bytes, from_mtef_bytes_with,
};

/// A xorshift generator.
struct Random(u64);

impl Random {
    fn below(&mut self, limit: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) % limit
    }

    fn one_of(&mut self, values: &[u16]) -> u16 {
        values[self.below(values.len() as u64) as usize]
    }
}

struct Format {
    v5: bool,
}

impl Format {
    fn character(&self, typeface: u8, code: u16, embellishments: &[u8]) -> Vec<u8> {
        let [low, high] = code.to_le_bytes();
        let has = u8::from(!embellishments.is_empty());
        let mut data = if self.v5 {
            vec![2, has]
        } else {
            vec![2 | has << 5]
        };
        data.extend([0x80 + typeface, low, high]);
        for &kind in embellishments {
            data.extend(if self.v5 { [6, 0, kind] } else { [6, kind, 0] });
        }
        if has != 0 {
            data.push(0);
        }
        data
    }

    fn line(&self, content: &[u8]) -> Vec<u8> {
        let tag: &[u8] = if self.v5 { &[1, 0] } else { &[1] };
        [tag, content, &[0]].concat()
    }

    fn template(&self, selector: u8, variation: u8, slots: &[Option<Vec<u8>>]) -> Vec<u8> {
        let tag: &[u8] = if self.v5 { &[3, 0] } else { &[3] };
        let mut data = [tag, &[selector, variation, 0]].concat();
        for slot in slots {
            match slot {
                Some(content) => data.extend(self.line(content)),
                None if self.v5 => data.extend([1, 1]),
                None => data.push(0x11),
            }
        }
        data.push(0);
        data
    }

    fn pile(&self, lines: &[Vec<u8>]) -> Vec<u8> {
        let tag: &[u8] = if self.v5 { &[4, 0] } else { &[4] };
        let mut data = [tag, &[1, 1]].concat();
        for line in lines {
            data.extend(self.line(line));
        }
        data.push(0);
        data
    }

    fn items(&self, random: &mut Random, depth: u64) -> Vec<u8> {
        let count = 1 + random.below(if depth == 0 { 6 } else { 3 });
        (0..count).flat_map(|_| self.item(random, depth)).collect()
    }

    fn item(&self, random: &mut Random, depth: u64) -> Vec<u8> {
        let embellishments = |random: &mut Random| -> Vec<u8> {
            (0..random.below(4))
                .map(|_| random.one_of(&[2, 3, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 20]) as u8)
                .collect()
        };
        let slot = |random: &mut Random, empty: u64| {
            (depth <= 4 && random.below(empty) != 0).then(|| self.items(random, depth + 1))
        };
        // The same templates have other numbers in MTEF 3.
        let of = |v5: u8, v3: u8| if self.v5 { v5 } else { v3 };
        match random.below(if depth > 4 { 8 } else { 24 }) {
            0 => self.character(3, u16::from(b'x'), &[]),
            1 | 2 => {
                let marks = embellishments(random);
                self.character(2, u16::from(b'\''), &marks)
            },
            3 => {
                let marks = embellishments(random);
                let letter = random.one_of(&[0x66, 0x3b1]);
                self.character(3, letter, &marks)
            },
            4 => {
                let marks = embellishments(random);
                self.character(6, 0x2032, &marks)
            },
            5 => {
                let marks = embellishments(random);
                self.character(1, u16::from(b'a'), &marks)
            },
            6 => self.character(8, u16::from(b'1'), &[]),
            7 => {
                let sign = random.one_of(&[0x3d, 0x2b, 0x28, 0x29, 0x222b, 0x7e, 0x2193]);
                self.character(6, sign, &[])
            },
            8..=13 => {
                let (sub, sup) = (slot(random, 9), slot(random, 9));
                match (self.v5, random.below(3)) {
                    (true, 0) => self.template(27, random.below(2) as u8, &[sub, None]),
                    (true, 1) => self.template(28, random.below(2) as u8, &[None, sup]),
                    (true, _) => self.template(29, 0, &[sub, sup]),
                    (false, 0) => self.template(15, 1, &[sub, None]),
                    (false, 1) => self.template(15, 0, &[None, sup]),
                    (false, _) => self.template(15, 2, &[sub, sup]),
                }
            },
            14 | 15 => {
                let mut slots = vec![slot(random, 5), slot(random, 3), slot(random, 3)];
                if self.v5 {
                    slots.push(Some(self.character(6, 0x2211, &[])));
                }
                self.template(of(21, 29), of(0, 2), &slots)
            },
            16 | 17 => {
                let slots = [slot(random, 9), slot(random, 3)];
                self.template(of(23, 39), 0, &slots)
            },
            18 => {
                let slots = [slot(random, 9), slot(random, 9)];
                self.template(of(10, 13), 1, &slots)
            },
            19 | 20 => {
                let slots = [slot(random, 9), slot(random, 9)];
                self.template(of(11, 14), random.below(2) as u8, &slots)
            },
            21 => {
                let slots = [slot(random, 9)];
                self.template(1, of(3, 0), &slots)
            },
            22 => {
                let lines = [self.items(random, depth + 1), self.items(random, depth + 1)];
                self.pile(&lines)
            },
            _ => {
                let content = self.items(random, depth + 1);
                self.line(&content)
            },
        }
    }

    fn equation(&self, content: &[u8], full: bool) -> Vec<u8> {
        let mut data = if self.v5 {
            [&[5, 1, 0, 6, 0][..], b"DSMT6\0\0"].concat()
        } else {
            vec![3, 1, 1, 3, 0]
        };
        if full {
            data.push(10);
        }
        data.extend(self.line(content));
        data.push(0);
        data
    }
}

#[track_caller]
fn has_no_sizes(data: &[u8], name: &str) {
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let Ok(read) = from_mtef_bytes_with(data, &lenient) else {
        return;
    };
    assert!(
        !read.compact_expr().arena.has_attrs(),
        "{name}: {:?}",
        read.expr()
    );
    assert!(read.expr().meta.base_size_pt.is_none(), "{name}");
    for version in [MtefVersion::V3, MtefVersion::V5] {
        if let Ok(bytes) = read.to_mtef_bytes(&MtefWriteOptions::new(version)) {
            let again = from_mtef_bytes(&bytes).unwrap();
            assert!(
                !again.compact_expr().arena.has_attrs(),
                "{name} {version:?}"
            );
        }
    }
}

#[test]
fn generated_equations_have_no_sizes() {
    for seed in 0..3_000u64 {
        let format = Format { v5: seed % 2 == 0 };
        let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let content = format.items(&mut random, 0);
        let full = random.below(2) == 0;
        has_no_sizes(&format.equation(&content, full), &format!("seed {seed}"));
    }
}

/// Every embellishment, and every two of them, on a prime, a letter and text
/// in a script, a script of a script, a limit and a root's index.
#[test]
fn embellishments_in_smaller_slots_have_no_sizes() {
    for v5 in [false, true] {
        let format = Format { v5 };
        let kinds: Vec<u8> = (2..=20).collect();
        for &(typeface, code) in &[(2, 0x27), (6, 0x2032), (3, 0x66), (1, 0x61)] {
            for &first in &kinds {
                for second in [None].into_iter().chain(kinds.iter().copied().map(Some)) {
                    let marks: Vec<u8> = [Some(first), second].into_iter().flatten().collect();
                    let letter = format.character(typeface, code, &marks);
                    let x = format.character(3, 0x78, &[]);
                    let sub = |content: Vec<u8>| {
                        if v5 {
                            format.template(27, 0, &[Some(content), None])
                        } else {
                            format.template(15, 1, &[Some(content), None])
                        }
                    };
                    let (limit, root) = if v5 { (23, 10) } else { (39, 13) };
                    for content in [
                        sub(letter.clone()),
                        sub([&x[..], &letter].concat()),
                        sub([&x[..], &sub(letter.clone())].concat()),
                        format.template(limit, 0, &[Some(x.clone()), Some(letter.clone())]),
                        format.template(root, 1, &[Some(x.clone()), Some(letter.clone())]),
                    ] {
                        let content = [&x[..], &content].concat();
                        let name = format!("{typeface} {code:x} {marks:?} v5 {v5}");
                        has_no_sizes(&format.equation(&content, true), &name);
                    }
                }
            }
        }
    }
}

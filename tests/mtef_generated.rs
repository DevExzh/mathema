//! Generated MTEF: primes, scripts on nothing and before something, colours,
//! zero spaces and templates without operators or limits, in every mix.
//!
//! Writing settles after one pass (the bytes of the second are those of the
//! third), and what is flat is read however long it is.

use mathema::ast::compact::CompactExpr;
use mathema::reader::mtef::MtefReader;
use mathema::writer::{Writer, mtef::MtefWriter};
use mathema::{Config, MtefVersion, MtefWriteOptions, Strictness, from_mtef_bytes_with};

const VERSIONS: [MtefVersion; 2] = [MtefVersion::V5, MtefVersion::V3];

/// A xorshift generator.
struct Random(u64);

impl Random {
    fn below(&mut self, limit: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) % limit
    }
}

fn line(content: &[u8]) -> Vec<u8> {
    [&[1, 0], content, &[0]].concat()
}

/// A character of a typeface with embellishments.
fn character(typeface: u8, code: u16, embellishments: &[u8]) -> Vec<u8> {
    let [low, high] = code.to_le_bytes();
    let mut data = vec![
        2,
        u8::from(!embellishments.is_empty()),
        0x80 + typeface,
        low,
        high,
    ];
    if !embellishments.is_empty() {
        for &kind in embellishments {
            data.extend([6, 0, kind]);
        }
        data.push(0);
    }
    data
}

fn template(selector: u8, variation: u8, slots: &[Option<Vec<u8>>]) -> Vec<u8> {
    let mut data = vec![3, 0, selector, variation, 0];
    for slot in slots {
        match slot {
            Some(content) => data.extend(line(content)),
            None => data.extend([1, 1]),
        }
    }
    data.push(0);
    data
}

fn equation(content: &[u8]) -> Vec<u8> {
    let mut data = vec![5, 1, 0, 6, 0];
    data.extend(b"DSMT6\0\0");
    for color in [[0u16, 0, 1000], [0, 0, 0], [1000, 0, 0]] {
        data.extend([16, 0]);
        for component in color {
            data.extend(component.to_le_bytes());
        }
    }
    data.push(10);
    data.extend(line(content));
    data.push(0);
    data
}

const X: (u8, u16) = (3, 'x' as u16);
const PRIME: (u8, u16) = (2, '\'' as u16);
const ZERO_SPACE: (u8, u16) = (24, 0xEF01);

fn plain((typeface, code): (u8, u16)) -> Vec<u8> {
    character(typeface, code, &[])
}

fn items(random: &mut Random, depth: u64) -> Vec<u8> {
    let count = 1 + random.below(if depth == 0 { 7 } else { 4 });
    (0..count).flat_map(|_| item(random, depth)).collect()
}

fn item(random: &mut Random, depth: u64) -> Vec<u8> {
    let slot = |random: &mut Random, empty: u64| {
        (depth <= 3 && random.below(empty) != 0).then(|| items(random, depth + 1))
    };
    match random.below(if depth > 3 { 14 } else { 26 }) {
        0 => plain(X),
        1 => plain((3, 's' as u16)),
        2 | 3 => plain(PRIME),
        4 => plain(ZERO_SPACE),
        5 => plain((8, '1' as u16)),
        6 => plain((1, 'a' as u16)),
        7 => plain((6, '=' as u16)),
        8 => plain((2, 's' as u16)),
        9 | 10 => vec![15, 1 + random.below(3) as u8],
        11 => character(2, PRIME.1, &[[5, 6, 18][random.below(3) as usize]]),
        12 => plain((6, 0x2032)),
        13 => plain((
            6,
            [0x28, 0x29, 0x222B, 0x2211, 0x2C][random.below(5) as usize],
        )),
        14 | 15 => {
            let sub = slot(random, 9);
            template(27, 0, &[sub, None])
        },
        16 | 17 => {
            let sub = slot(random, 9);
            template(27, 1, &[sub, None])
        },
        18 | 19 => {
            let sup = slot(random, 9);
            template(28, random.below(2) as u8, &[None, sup])
        },
        20 => {
            let (sub, sup) = (slot(random, 9), slot(random, 9));
            template(29, 0, &[sub, sup])
        },
        21 | 22 => {
            let (body, operator) = (slot(random, 4), slot(random, 3));
            template(21, 0, &[body, None, None, operator])
        },
        23 => {
            let body = slot(random, 9);
            template(23, 0x30, &[body, None, None])
        },
        24 => {
            let top = slot(random, 9);
            template(11, 0, &[top, Some(plain((3, 'y' as u16)))])
        },
        _ => line(&items(random, depth + 1)),
    }
}

fn generated(seed: u64) -> Vec<u8> {
    let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut content = items(&mut random, 0);
    if random.below(12) == 0 {
        for _ in 0..random.below(60) {
            content = [&[4, 0, 2, 1][..], &line(&content), &[0]].concat();
        }
    }
    equation(&content)
}

/// Why writing `data` again and again does not settle.
fn unsettled(data: &[u8]) -> Option<String> {
    let strict = Config::default();
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let first = from_mtef_bytes_with(data, &lenient).ok()?;
    for version in VERSIONS {
        let options = MtefWriteOptions::new(version);
        // (Too deep to be written.)
        let Ok(one) = first.to_mtef_bytes(&options) else {
            continue;
        };
        let mut passes = vec![one];
        for _ in 0..2 {
            let read = match from_mtef_bytes_with(&passes[passes.len() - 1], &strict) {
                Ok(read) => read,
                Err(e) => return Some(format!("{version:?}: {e}")),
            };
            match read.to_mtef_bytes_with_diagnostics(&options) {
                Ok((_, lost)) if lost.iter().any(|d| d.message.contains("math atom")) => {
                    return None;
                },
                Ok((bytes, _)) => passes.push(bytes),
                Err(e) => return Some(format!("{version:?}: {e}")),
            }
        }
        if passes[1] != passes[2] {
            return Some(format!("{version:?}: not idempotent"));
        }
    }
    None
}

#[test]
fn generated_equations_settle() {
    for seed in 0..20_000 {
        if let Some(reason) = unsettled(&generated(seed)) {
            panic!("seed {seed}: {reason}");
        }
    }
}

/// The writer gives the same for the heap tree as for the compact one, as it
/// was read and normalized.
#[test]
fn generated_equations_are_written_alike_from_heap_and_compact() {
    let config = Config::default();
    for seed in 0..5_000 {
        let Ok(read) = MtefReader::parse_bytes(&generated(seed), &config) else {
            continue;
        };
        let mut normalized = read.expr.clone();
        mathema::ast::normalize::normalize_expr(&mut normalized);
        for expr in [&read.expr, &normalized] {
            let Ok(compact) = CompactExpr::from_math_expr(expr) else {
                continue;
            };
            assert_eq!(
                MtefWriter::write(expr, &config).ok(),
                MtefWriter::write_compact(&compact, &config).ok(),
                "seed {seed}"
            );
        }
    }
}

/// Every list of up to two kinds of items, repeated: read and settled, with
/// no nesting in the source none in the tree that outgrows its limit.
#[test]
fn flat_equations_are_read_however_long() {
    let x = plain(X);
    let prime = plain(PRIME);
    let script = |selector, variation, content: &[u8]| {
        template(selector, variation, &[Some(content.to_vec()), None])
    };
    let sub = script(27, 0, &x);
    let drawn = |body: &[u8]| template(21, 0, &[Some(vec![]), None, None, Some(body.to_vec())]);
    let limit = |body: &[u8]| template(23, 0x30, &[Some(body.to_vec()), None, None]);
    let primed = [&prime[..], &sub].concat();
    let kinds: Vec<Vec<u8>> = vec![
        x.clone(),
        prime.clone(),
        sub.clone(),
        script(27, 1, &x),
        vec![15, 1],
        vec![15, 2],
        plain(ZERO_SPACE),
        character(2, PRIME.1, &[18]),
        drawn(&prime),
        drawn(&primed),
        drawn(&[&primed[..], &sub].concat()),
        limit(&[]),
        limit(&prime),
        limit(&primed),
        limit(&sub),
        line(&plain(ZERO_SPACE)),
        plain((6, '=' as u16)),
    ];
    for first in &kinds {
        for second in &kinds {
            // Scripts on scripts only are nested.
            let unit = [&x[..], first, second].concat();
            let data = equation(&unit.repeat(200));
            let read = from_mtef_bytes_with(&data, &Config::default());
            assert!(read.is_ok(), "{first:?} {second:?}: {:?}", read.err());
            assert_eq!(unsettled(&data), None, "{first:?} {second:?}");
            // With nothing new to carry the primes and scripts.
            let unit = [&first[..], second, &prime].concat();
            let data = equation(&[&[15, 1][..], &x, &unit.repeat(200)].concat());
            if from_mtef_bytes_with(&data, &Config::default()).is_ok() {
                assert_eq!(unsettled(&data), None, "{first:?} {second:?}");
            }
        }
    }
    // Thousands of primed letters, of primes on one, and of scripts and
    // primes in two colours.
    for unit in [
        character(2, PRIME.1, &[18]),
        [&character(2, PRIME.1, &[18])[..], &prime, &prime].concat(),
        prime.clone(),
    ] {
        let data = equation(&[&x[..], &unit.repeat(60_000 / unit.len() * 5)].concat());
        let read = from_mtef_bytes_with(&data, &Config::default());
        assert!(read.is_ok(), "{unit:?}: {:?}", read.err());
        assert_eq!(unsettled(&data), None, "{unit:?}");
    }
    let blue_prime = [&[15, 1][..], &prime, &[15, 2], &sub].concat();
    for unit in [
        [&x[..], &prime].concat(),
        prime.clone(),
        blue_prime,
        // An empty limit template, and a line of a zero space, between
        // scripts: they have bases of their own.
        [&sub[..], &limit(&[])].concat(),
        [&sub[..], &line(&plain(ZERO_SPACE))].concat(),
    ] {
        let data = equation(&[&[15, 1][..], &x, &unit.repeat(5_000)].concat());
        let read = from_mtef_bytes_with(&data, &Config::default());
        assert!(read.is_ok(), "{unit:?}: {:?}", read.err());
        assert_eq!(unsettled(&data), None, "{unit:?}");
    }
}

/// A variable `s` with primes of its own after a prime is no possessive.
#[test]
fn a_primed_s_after_a_prime_is_a_variable() {
    for mark in [
        character(2, PRIME.1, &[18]),
        plain(PRIME),
        plain((6, 0x2032)),
    ] {
        let data = equation(&[plain(X), plain(PRIME), plain((3, 's' as u16)), mark].concat());
        let mathml = from_mtef_bytes_with(&data, &Config::default())
            .unwrap()
            .to_mathml()
            .unwrap();
        assert_eq!(mathml.matches("<msup>").count(), 2, "{mathml}");
        assert_eq!(unsettled(&data), None);
    }
}

/// Equations and the colour selected for each character drawn.
struct Coloured {
    random: Random,
    color: u8,
    drawn: Vec<(char, u8)>,
}

impl Coloured {
    fn draw(&mut self, what: (u8, u16)) -> Vec<u8> {
        let ch = char::from_u32(what.1.into()).unwrap();
        self.drawn
            .push((if ch == '\'' { '′' } else { ch }, self.color));
        plain(what)
    }

    fn items(&mut self, depth: u64) -> Vec<u8> {
        let count = if depth == 0 {
            2 + self.random.below(6)
        } else {
            1 + self.random.below(2)
        };
        (0..count).flat_map(|_| self.item(depth)).collect()
    }

    fn item(&mut self, depth: u64) -> Vec<u8> {
        let mut data = Vec::new();
        if self.random.below(3) != 0 {
            self.color = 1 + self.random.below(3) as u8;
            data.extend([15, self.color]);
        }
        let kind = self.random.below(if depth > 1 { 10 } else { 19 });
        data.extend(match kind {
            0 | 1 => {
                let letter = [X.1, 's' as u16, 'y' as u16][self.random.below(3) as usize];
                self.draw((3, letter))
            },
            2 | 3 => self.draw(PRIME),
            4 => self.draw((8, '1' as u16)),
            5 => "sin"
                .chars()
                .flat_map(|ch| self.draw((2, ch as u16)))
                .collect(),
            6 => self.draw((6, '+' as u16)),
            7 => {
                // A prime and two primes over it.
                self.drawn.extend([('′', self.color); 3]);
                character(2, PRIME.1, &[6])
            },
            8 => plain(ZERO_SPACE),
            9 => self.draw((1, 'a' as u16)),
            10 | 11 => template(27, 0, &[Some(self.items(depth + 1)), None]),
            12 => template(28, 0, &[None, Some(self.items(depth + 1))]),
            13 | 14 => template(27, 1, &[Some(self.items(depth + 1)), None]),
            15 => template(28, 1, &[None, Some(self.items(depth + 1))]),
            16 => template(23, 0x30, &[Some(self.items(depth + 1)), None, None]),
            17 => template(
                21,
                0,
                &[Some(vec![]), None, None, Some(self.items(depth + 1))],
            ),
            _ => {
                let top = self.items(depth + 1);
                template(11, 0, &[Some(top), Some(self.items(depth + 1))])
            },
        });
        data
    }
}

/// The characters of MathML and their colours.
fn colours_drawn(mathml: &str) -> Vec<(char, u8)> {
    let mut drawn = Vec::new();
    let mut around = vec![2];
    for part in mathml.split('<').skip(1) {
        let (tag, text) = part.split_once('>').unwrap();
        if tag.starts_with('/') {
            around.pop();
        } else if !tag.ends_with('/') {
            let color = match tag.split_once("mathcolor=\"").map(|(_, value)| &value[..7]) {
                Some("#0000ff") => 1,
                Some("#000000") => 2,
                Some("#ff0000") => 3,
                _ => around[around.len() - 1],
            };
            around.push(color);
        }
        let text = text.replace("&apos;", "′");
        let color = around.last().copied().unwrap_or(2);
        drawn.extend(
            text.chars()
                .filter(|ch| !ch.is_whitespace() && !('\u{2061}'..='\u{2064}').contains(ch))
                .map(|ch| (ch, color)),
        );
    }
    drawn.sort_unstable();
    drawn
}

/// Every character has the colour that was selected when it was drawn, as it
/// is read and when it was written and read again.
#[test]
fn generated_colours_are_those_selected() {
    for seed in 0..30_000u64 {
        let mut coloured = Coloured {
            random: Random((seed + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
            color: 2,
            drawn: Vec::new(),
        };
        let content = coloured.items(0);
        coloured.drawn.sort_unstable();
        let data = equation(&[&[15, 2][..], &content].concat());
        let Ok(read) = from_mtef_bytes_with(&data, &Config::default()) else {
            continue;
        };
        let mathml = read.to_mathml().unwrap();
        assert_eq!(
            colours_drawn(&mathml),
            coloured.drawn,
            "seed {seed}: {mathml}"
        );
        let Ok(written) = read.to_mtef_bytes(&MtefWriteOptions::default()) else {
            continue;
        };
        let again = from_mtef_bytes_with(&written, &Config::default()).unwrap();
        let mathml = again.to_mathml().unwrap();
        assert_eq!(
            colours_drawn(&mathml),
            coloured.drawn,
            "seed {seed}, rewritten: {mathml}"
        );
    }
}

/// Coloured parts of templates that join the lines around are painted again
/// in each: not without bound.
#[test]
fn coloured_templates_in_templates_are_read() {
    let item = [&[15, 1][..], &plain(X), &[15, 3], &plain(PRIME)].concat();
    let mut content = item.repeat(10_000);
    for _ in 0..60 {
        content = template(23, 0x30, &[Some(content), None, None]);
    }
    let read = from_mtef_bytes_with(&equation(&content), &Config::default());
    assert!(read.is_ok(), "{:?}", read.err());
}

/// Primes of both spellings share the bound of one item.
#[test]
fn many_primes_of_two_spellings_are_read() {
    let content = [
        plain(X),
        character(2, PRIME.1, &[18]).repeat(7_000),
        plain(PRIME).repeat(40_000),
    ]
    .concat();
    let data = equation(&content);
    let read = from_mtef_bytes_with(&data, &Config::default());
    assert!(read.is_ok(), "{:?}", read.err());
    assert_eq!(unsettled(&data), None);
}

/// Colours found wrong in generated equations.
#[test]
fn reduced_generated_colours_are_those_selected() {
    let items = |parts: &[Vec<u8>]| parts.concat();
    let color = |index: u8| vec![15, index];
    let limit = |body: Vec<u8>| template(23, 0x30, &[Some(body), None, None]);
    let primes = character(2, PRIME.1, &[6]);
    let a = plain((1, 'a' as u16));
    let one = plain((8, '1' as u16));
    // A leading script before a primed letter in another colour.
    let blue_script = template(27, 0, &[Some(a.clone()), None]);
    let drawn = template(
        21,
        0,
        &[
            Some(vec![]),
            None,
            None,
            Some(items(&[color(2), primes.clone()])),
        ],
    );
    let sup = template(
        28,
        0,
        &[
            None,
            Some(items(&[color(2), plain(ZERO_SPACE), color(1), one.clone()])),
        ],
    );
    let opened = items(&[
        limit(items(&[color(1), blue_script])),
        limit(items(&[color(2), drawn, color(1), sup])),
    ]);
    // Primes joined one by one under a leading script, then a subscript.
    let joined = items(&[
        limit(items(&[
            template(27, 0, &[Some(a.clone()), None]),
            one.clone(),
        ])),
        primes.clone(),
        plain(PRIME),
        primes,
        template(27, 0, &[Some(items(&[color(3), plain(X), color(2)])), None]),
        color(2),
    ]);
    // A generated equation: marks joined to a row that is painted before the
    // line ends.
    let digits = concat!(
        "0f02030017000001000f03020083780002018227000600060000010101010003",
        "00170000010003001b000001000f020200816100000101000200883100000101",
        "0101000f020201822700060006000f0202008227000201822700060006000300",
        "1b000001000f0203001b000001000f0302008378000001010000010100",
    );
    let generated: Vec<u8> = (0..digits.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).unwrap())
        .collect();
    for (content, expected) in [
        (generated, {
            let mut expected = vec![('1', 2), ('a', 2), ('x', 3), ('x', 3)];
            expected.extend([('′', 2); 7]);
            expected.extend([('′', 3); 3]);
            expected
        }),
        (
            opened,
            vec![('1', 1), ('a', 1), ('′', 2), ('′', 2), ('′', 2)],
        ),
        (joined, {
            let mut expected = vec![('1', 2), ('a', 2), ('x', 3)];
            expected.extend([('′', 2); 7]);
            expected
        }),
    ] {
        let data = equation(&[&[15, 2][..], &content].concat());
        let mathml = from_mtef_bytes_with(&data, &Config::default())
            .unwrap()
            .to_mathml()
            .unwrap();
        let mut expected = expected;
        expected.sort_unstable();
        assert_eq!(colours_drawn(&mathml), expected, "{mathml}");
        assert_eq!(unsettled(&data), None, "{mathml}");
    }
}

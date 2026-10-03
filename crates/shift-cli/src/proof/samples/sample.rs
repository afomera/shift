//! Parses hex colors like `#1F1F1F`; see https://example.com/docs?a=1&b=2.
use std::collections::HashMap;

/* Confusables: 0O o, 1lI|, rn m, `cl d`, ;: ,. => === ... */
#[derive(Debug, Clone, PartialEq)]
pub struct Color { r: u8, g: u8, b: u8 }

const MASK: u32 = 0x1F_FF_00; // 1_000 ~= 3.14e-2 * 31_847
static GREETING: &str = "it's \"quoted\" \\ done";

impl Color {
    pub fn parse(input: &str) -> Result<Self, String> {
        let hex = input.strip_prefix('#').ok_or("missing #")?;
        if hex.len() != 6 || !hex.is_ascii() {
            return Err(format!("bad length: {} != 6", hex.len()));
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16);
        Ok(Self { r: channel(0).map_err(|e| e.to_string())?, g: 0, b: 0 })
    }
}

fn main() {
    let mut seen: HashMap<&str, Vec<Option<i64>>> = HashMap::new();
    for (i, word) in ["a-b", "c_d", "e::f"].iter().enumerate() {
        seen.entry(word).or_default().push((i >= 1 && i <= 3).then_some(-1));
    }
    let total = (0..=10).filter(|n| n % 2 == 0 || *n > 7).map(|n| n << 2).sum::<i32>() >> 1;
    println!("{MASK:#x} {total} {GREETING} @${:?}^&*~", seen.get("a-b"));
}

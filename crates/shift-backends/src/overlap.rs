//! Marks compiled glyphs as possibly overlapping.
//!
//! Shift sources keep overlapping contours (slabs crossing stems, bars
//! crossing bowls), and removing them would change point counts between
//! masters. Instead every glyph carries the TrueType overlap flag, as
//! fontmake and fontTools' instancer do: `OVERLAP_SIMPLE` on the first point
//! flag of simple glyphs and `OVERLAP_COMPOUND` on the first component of
//! composites. Apple's rasterizer and some PDF/print engines rely on it to
//! fill overlaps without seams or darker spots, especially between masters.
//!
//! The flag can grow a glyph by a byte or two (when the first point flag is a
//! repeat run it has to be split), so `glyf` and `loca` are rewritten and
//! `loca` always uses the long format.

use write_fonts::read::{FontRef, TableProvider};
use write_fonts::types::Tag;
use write_fonts::FontBuilder;

const ON_REPEAT: u8 = 0x08;
const OVERLAP_SIMPLE: u8 = 0x40;
const OVERLAP_COMPOUND: u16 = 0x0400;
/// `indexToLocFormat` offset in `head`.
const HEAD_LOC_FORMAT: usize = 50;

/// Returns `font` with every non-empty glyph flagged as overlapping. Fonts
/// without `glyf` come back unchanged.
pub(crate) fn flag_overlaps(font: &[u8]) -> Result<Vec<u8>, String> {
    let font_ref = FontRef::new(font).map_err(|error| error.to_string())?;
    let (Some(glyf), Some(head)) = (
        font_ref.table_data(Tag::new(b"glyf")),
        font_ref.table_data(Tag::new(b"head")),
    ) else {
        return Ok(font.to_vec());
    };
    let loca = font_ref.loca(None).map_err(|error| error.to_string())?;
    let num_glyphs = font_ref
        .maxp()
        .map_err(|error| error.to_string())?
        .num_glyphs() as usize;
    let glyf = glyf.as_bytes();

    let mut new_glyf = Vec::with_capacity(glyf.len() + num_glyphs * 4);
    let mut offsets = Vec::with_capacity(num_glyphs + 1);
    for gid in 0..num_glyphs {
        offsets.push(new_glyf.len() as u32);
        let start = loca.get_raw(gid).ok_or("loca is shorter than maxp")? as usize;
        let end = loca.get_raw(gid + 1).ok_or("loca is shorter than maxp")? as usize;
        let data = glyf
            .get(start..end)
            .ok_or_else(|| format!("glyph {gid} lies outside glyf"))?;
        flag_glyph(data, &mut new_glyf).map_err(|error| format!("glyph {gid}: {error}"))?;
        new_glyf.resize(new_glyf.len().next_multiple_of(4), 0);
    }
    offsets.push(new_glyf.len() as u32);

    let mut new_head = head.as_bytes().to_vec();
    new_head
        .get_mut(HEAD_LOC_FORMAT..HEAD_LOC_FORMAT + 2)
        .ok_or("head is truncated")?
        .copy_from_slice(&1i16.to_be_bytes());
    let new_loca: Vec<u8> = offsets
        .iter()
        .flat_map(|offset| offset.to_be_bytes())
        .collect();

    Ok(FontBuilder::new()
        .add_raw(Tag::new(b"glyf"), new_glyf)
        .add_raw(Tag::new(b"loca"), new_loca)
        .add_raw(Tag::new(b"head"), new_head)
        .copy_missing_tables(font_ref)
        .build())
}

/// Appends `data` (one glyph) to `out` with its overlap flag set.
fn flag_glyph(data: &[u8], out: &mut Vec<u8>) -> Result<(), &'static str> {
    if data.len() < 10 {
        out.extend_from_slice(data); // empty glyph
        return Ok(());
    }
    let contours = i16::from_be_bytes([data[0], data[1]]);
    if contours < 0 {
        // Composite: the first component's flags follow the header.
        let mut glyph = data.to_vec();
        let flags = u16::from_be_bytes([glyph[10], glyph[11]]) | OVERLAP_COMPOUND;
        glyph[10..12].copy_from_slice(&flags.to_be_bytes());
        out.extend_from_slice(&glyph);
        return Ok(());
    }
    if contours == 0 {
        out.extend_from_slice(data);
        return Ok(());
    }

    let ends = 10 + 2 * contours as usize;
    let read_u16 = |at: usize| {
        data.get(at..at + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
            .ok_or("truncated simple glyph")
    };
    let points = read_u16(ends - 2)? + 1;
    let flags_start = ends + 2 + read_u16(ends)?;
    let first = *data.get(flags_start).ok_or("truncated point flags")?;

    out.extend_from_slice(&data[..flags_start]);
    if first & ON_REPEAT == 0 {
        out.push(first | OVERLAP_SIMPLE);
        out.extend_from_slice(&data[flags_start + 1..]);
        return Ok(());
    }
    // The first flag repeats: give the first point its own flag byte and let
    // the run carry on for the rest.
    let repeat = *data.get(flags_start + 1).ok_or("truncated point flags")?;
    if points <= repeat as usize {
        return Err("point flag run is longer than the glyph");
    }
    out.push((first & !ON_REPEAT) | OVERLAP_SIMPLE);
    match repeat {
        0 => {}
        1 => out.push(first & !ON_REPEAT),
        n => out.extend_from_slice(&[first, n - 1]),
    }
    out.extend_from_slice(&data[flags_start + 2..]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-contour square: 4 on-curve points whose flags are one repeat run.
    fn square(flags: &[u8]) -> Vec<u8> {
        let mut glyph = vec![0, 1, 0, 0, 0, 0, 0, 100, 0, 100]; // 1 contour, bbox
        glyph.extend_from_slice(&[0, 3, 0, 0]); // endPts [3], no instructions
        glyph.extend_from_slice(flags);
        glyph.extend_from_slice(&[0xAA; 6]); // coordinates, untouched
        glyph
    }

    #[test]
    fn splits_a_repeated_first_flag_so_only_the_first_point_is_marked() {
        let mut out = Vec::new();
        flag_glyph(&square(&[0x01 | ON_REPEAT, 3]), &mut out).unwrap();

        assert_eq!(out, square(&[0x01 | OVERLAP_SIMPLE, 0x01 | ON_REPEAT, 2]));
    }

    #[test]
    fn marks_a_plain_first_flag_in_place() {
        let mut out = Vec::new();
        flag_glyph(&square(&[0x01, 0x01, 0x01, 0x01]), &mut out).unwrap();

        assert_eq!(out, square(&[0x01 | OVERLAP_SIMPLE, 0x01, 0x01, 0x01]));
    }

    #[test]
    fn a_two_point_run_leaves_one_plain_flag() {
        let mut out = Vec::new();
        flag_glyph(&square(&[0x01 | ON_REPEAT, 1, 0x21, 0x21]), &mut out).unwrap();

        assert_eq!(out, square(&[0x01 | OVERLAP_SIMPLE, 0x01, 0x21, 0x21]));
    }

    #[test]
    fn marks_the_first_component_of_a_composite() {
        let composite = [0xFF, 0xFF, 0, 0, 0, 0, 0, 10, 0, 10, 0x00, 0x02, 0, 5, 0, 0];
        let mut out = Vec::new();
        flag_glyph(&composite, &mut out).unwrap();

        assert_eq!(&out[10..12], &(0x0002 | OVERLAP_COMPOUND).to_be_bytes());
        assert_eq!(&out[12..], &composite[12..]);
    }
}

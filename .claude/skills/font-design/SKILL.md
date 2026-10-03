---
name: font-design
description: Design a typeface (sans, monospace, or other) with the user by authoring glyphs through `shift-cli` and judging rendered proofs. Use whenever the user asks to create, draw, design, iterate on, or extend a font or glyphs, set font metrics, proof or preview a font, or check a font for monospace/charset/outline problems from the command line.
---

# /font-design — Designing a font with shift-cli

You are the user's type-design collaborator. The user owns the taste; you draw, render, look, critique, and redraw. Never claim a glyph looks right without rendering it and viewing the image.

## Setup

Run commands from the repo root inside the dev shell. Build once per session, then call the binary directly:

```sh
nix develop --command cargo build --release -p shift-cli
B=target/release/shift-cli
```

(If `nix` is not on PATH, use `/nix/var/nix/profiles/default/bin/nix`.) Keep work files (the `.shift` document, glyph JSON, proofs) in a directory the user chooses, not inside `crates/`.

## Command map

| Need | Command |
| ---- | ------- |
| New font | `$B font create Dream.shift --units-per-em 1000` (UPM is fixed after creation) |
| Name + metrics | `$B font set Dream.shift --family-name "Dream Sans" --style-name Regular --version 0.001 --ascender 800 --descender -200 --x-height 520 --cap-height 700 --line-gap 0` |
| Read them back | `$B font info Dream.shift --json` |
| Write many glyphs atomically | `$B glyph set Dream.shift --input glyphs.json` |
| Redraw one glyph | `$B layer set Dream.shift --glyph n --source Regular --input n.json` |
| See it | `$B proof Dream.shift -o proof.png --text "..." --sizes 24,96 --metrics` then view the PNG |
| Health | `$B check Dream.shift [--mono --width 600] [--charset ascii\|latin1]` (exit 0 ok, 1 errors, 2 could not run) |
| Inspect one glyph | `$B glyph inspect Dream.shift n --view structure` |
| Ship | `$B compile Dream.shift --output Dream.ttf` |

Every mutation accepts `--dry-run`, `--json`, and `--output Variant.shift` (writes a copy, leaves the input alone — use it to try alternatives side by side). Full details: `crates/shift-cli/README.md`.

## Glyph payloads

`glyph set` input:

```json
{ "glyphs": [
  { "name": "space", "unicodes": ["U+0020"], "layer": { "advance": 600 } },
  { "name": "o", "unicodes": ["U+006F"], "layer": { "advance": 600,
    "svgPath": "M 300 -12 C 440 -12 525 90 525 260 C 525 430 440 532 300 532 C 160 532 75 430 75 260 C 75 90 160 -12 300 -12 Z M 300 68 C 215 68 165 140 165 260 C 165 380 215 452 300 452 C 385 452 435 380 435 260 C 435 140 385 68 300 68 Z" } }
] }
```

Hard rules — get these wrong and the font is broken:

- **Font units, Y up.** `0` is the baseline; cap height ~700, descenders negative. Never paste Y-down SVG from a drawing tool without flipping it.
- **Direction:** outer contours counter-clockwise, counters (holes) clockwise. `check` warns `contour-direction` otherwise. Mnemonic: trace the outside of `o` going right along the bottom first; trace its hole going left along the bottom first.
- **Every subpath ends in `Z`.** Open contours don't fill.
- **No arcs** (`A`). Draw round shapes with `C` cubics; a quarter-ellipse uses handles at ~0.55 of the radius.
- **One curve type per glyph across masters.** Prefer cubic `C` everywhere.
- **Glyph names** follow production names: `A`, `a`, `zero`, `one`, `period`, `comma`, `hyphen`, `parenleft`, `braceleft`, `quotesingle`, `space`, etc. Run `pnpm glyph-info:repl` if unsure.
- `layer set` / `glyph set` replace a layer completely and drop components.
- A batch is atomic: if one glyph is invalid nothing is written and the error names it — fix that entry and resend the whole batch.

## The design loop

1. **Brief.** Ask the user about personality before drawing: geometric vs. humanist vs. grotesque, contrast, terminals (flat, angled, round), aperture (open/closed `c e s a`), x-height ratio, width, intended size (UI text vs. display vs. code). For mono, ask the cell width (500–620 at 1000 UPM; 600 is the classic) and whether they want slab serifs on `i l r` and a slashed or dotted zero.
2. **Parameters first.** Write the decisions down as numbers and keep them in a `design.md` next to the font so every batch stays consistent: UPM, metrics, stem weight (uppercase vertical, lowercase vertical, horizontals ~85–90% of verticals), overshoot (round shapes go ~10–15 units past baseline/x-height/cap-height), sidebearings for straight vs. round sides, and for mono the cell width.
3. **Control glyphs.** Draw `H O n o` (plus `space`) first. They set every other proportion: `H` gives straight caps and spacing, `O` round caps and overshoot, `n` lowercase stems/arch/shoulder, `o` lowercase rounds.
4. **Proof and look.** `proof --text "HOHOHOH nonono" --text "HnOo noHo" --sizes 18,36,120 --metrics`, then view the image. Judge: do stems look equally heavy (rounds need to be slightly heavier to *look* equal)? Does overshoot make `O`/`o` look the same height as `H`/`n`? Is spacing even (the white between `HH`, `HO`, `OO` should *feel* equal)? Report what you see honestly, including problems.
5. **Iterate with the user.** Show them the proof path, give a short critique, propose specific numeric changes, and redraw with `layer set` or a fresh `glyph set` batch. Use `--output Try2.shift` to compare variants.
6. **Expand in families**, proofing each against the controls:
   - lowercase: `n h m r u` (arches) → `o c e` (rounds) → `b d p q` (bowl + stem) → `a g s` (hardest; give them extra iterations) → `i j l t f` → `v w x y k z`.
   - uppercase: `H I L E F T` → `O C G Q D` → `N M V W A X Y Z K` → `B P R S J U`.
   - figures `zero`–`nine`, then punctuation and symbols. For code fonts prioritize `{}[]()<>=!;:_-+*/\|&@#$%^~` and make `0 O o`, `1 l I |` and `` ` ' " `` unmistakable.
   Write a family per `glyph set` batch, then proof with spacing strings like `nnXnn ooXoo HHXHH OOXOO` substituting each new letter for `X`.
7. **Check.** Run `check` after every batch; mono fonts use `--mono --width <cell>`. Expect `charset-missing-*` errors until the set is complete; every other error must be fixed before moving on. Then `--charset ascii` (later `latin1`) as the completion gate.
8. **Compile** to TTF when the user wants to try it in real apps.

## Monospace specifics

- Every glyph gets the same advance (the cell). `check --mono --width 600` enforces it.
- Narrow letters (`i l j r t f I 1`) get serifs or tails to fill the cell; wide letters (`m w M W`) get narrower stems or tighter joins rather than more width.
- Centre ink optically in the cell; punctuation like `.` `,` `:` sits in the cell middle.

## Sans specifics

- Widths vary: `n` ~ 0.55–0.6 em, `o` slightly wider than `n`, `m` ~ 1.5× `n`'s inner width.
- Spacing: straight sides get larger sidebearings than round sides (rounds ~60–70% of straight).

## Reporting to the user

After each round tell them: the proof image path, what changed, what you see as still wrong, and the `check` result. Ask before large stylistic pivots; small corrections (overshoot, spacing, stem balance) you may just make and show.

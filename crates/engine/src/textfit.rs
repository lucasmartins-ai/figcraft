//! `text.fit`: text sized to fit its box, for posts and cards whose copy changes (a carousel
//! filled from data, a translated headline). The largest font size, down to a floor, at which the
//! text stays within `maxLines` lines and its box's height; then a last line that is a single word
//! (an orphan) is avoided by shrinking a little more, else by keeping the last two words together
//! with a no-break space. Every style run scales by the same factor, so mixed sizes keep their
//! proportions.

use figcraft_doc::*;
use serde_json::{Value, json};

use crate::cmd::CommandSpec;
use crate::params::*;
use crate::{Result, Session, bad, spec};

const CMD: &str = "text.fit";

pub fn specs() -> Vec<CommandSpec> {
    vec![spec!(
        "text.fit",
        "Fit text to its box",
        "{ids? (text layers; default the selection), maxLines? (default: no limit but the box height), minSize? (smallest font size, px; default 60% of the current), orphans? (avoid a one-word last line; default true)}  needs a fixed width (fixed or auto-height text)  → {fitted: [{id, fontSize, lines, fits, orphan}]}",
        true,
        fit
    )]
}

/// The layout of `t` with every run's font size times `k`, at `width`: (line count, height,
/// words on the last line).
fn measure(t: &TextData, k: f64, width: f64) -> (usize, f64, usize) {
    let mut t = t.clone();
    let n = t.char_count();
    t.restyle(0, n, |s| s.font_size = (s.font_size * k).clamp(1.0, 5000.0));
    let l = figcraft_text::layout(&t, Some(width), None);
    let last = l.lines.last().map_or(0, |line| {
        t.characters.chars().skip(line.char_start).take(line.char_end.saturating_sub(line.char_start)).collect::<String>().split_whitespace().count()
    });
    (l.lines.len(), l.height, last)
}

fn fit(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets(CMD, s, p)?;
    let max_lines = num(p, "maxLines").filter(|v| v.is_finite() && *v >= 1.0).map(|v| v.min(1000.0) as usize);
    let orphans = boolean(p, "orphans").unwrap_or(true);
    let mut out = Vec::new();
    let doc = s.doc_mut()?;
    for id in ids {
        let n = doc.node(id).ok_or_else(|| bad(CMD, format!("no layer {id}")))?;
        let NodeKind::Text(t) = &n.kind else { return Err(bad(CMD, format!("{id} is not text"))) };
        if t.auto_resize == TextAutoResize::WidthAndHeight {
            return Err(bad(CMD, format!("{id} grows to its text: give it a fixed width (auto height or fixed) first")));
        }
        let (width, height_cap) = (n.width, if t.auto_resize == TextAutoResize::None { n.height } else { f64::INFINITY });
        let size = t.style.font_size.max(1.0);
        let min = num(p, "minSize").filter(|v| v.is_finite() && *v >= 1.0).unwrap_or(size * 0.6).min(size);
        let fits = |m: (usize, f64, usize)| max_lines.is_none_or(|ml| m.0 <= ml) && m.1 <= height_cap + 0.5;
        // The largest factor in [min/size, 1] that fits: binary search to a tenth of a pixel.
        let (mut lo, mut hi) = (min / size, 1.0);
        let k = if fits(measure(t, 1.0, width)) {
            1.0
        } else if !fits(measure(t, lo, width)) {
            lo
        } else {
            for _ in 0..24 {
                let mid = (lo + hi) / 2.0;
                if fits(measure(t, mid, width)) {
                    lo = mid
                } else {
                    hi = mid
                }
                if (hi - lo) * size < 0.1 {
                    break;
                }
            }
            lo
        };
        // Whole-tenth sizes read better in the panel.
        let mut k = ((size * k * 10.0).floor() / 10.0) / size;
        let mut m = measure(t, k, width);
        let mut orphan = orphans && m.0 > 1 && m.2 == 1;
        let mut nbsp = false;
        if orphan {
            // Up to 8% smaller (not below the floor) may pull a word down to the last line.
            let mut tries = 0;
            while orphan && tries < 8 && size * k * 0.99 >= min {
                k *= 0.99;
                m = measure(t, k, width);
                orphan = m.0 > 1 && m.2 == 1;
                tries += 1;
            }
            if orphan {
                nbsp = true;
            }
        }
        let fitted = fits(m);
        let n = doc.get_mut(id)?;
        let NodeKind::Text(t) = &mut n.kind else { continue };
        let count = t.char_count();
        t.restyle(0, count, |st| st.font_size = ((st.font_size * k * 10.0).round() / 10.0).clamp(1.0, 5000.0));
        if nbsp && let Some(i) = t.characters.trim_end().rfind(' ') {
            // Same character count, so style runs keep their ranges.
            t.characters.replace_range(i..i + 1, "\u{a0}");
        }
        let after = measure(t, 1.0, width);
        out.push(json!({"id": id.0, "fontSize": (t.style.font_size * 10.0).round() / 10.0, "lines": after.0, "fits": fitted, "orphan": orphan && !nbsp && after.2 == 1}));
    }
    Ok(json!({"fitted": out}))
}

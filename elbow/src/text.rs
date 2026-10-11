//! Text width estimates for the fonts of the diagram-design style, without font files.

const MONO_ADVANCE_EM: f64 = 0.6;

/// Width of a Geist (sans) line at `size` px, semibold. Slightly generous so text never overflows its box.
pub fn sans_width(text: &str, size: f64) -> f64 {
    text.chars().map(sans_advance_em).sum::<f64>() * size
}

/// Width of a Geist Mono line at `size` px with `tracking_em` letter spacing.
pub fn mono_width(text: &str, size: f64, tracking_em: f64) -> f64 {
    text.chars().count() as f64 * size * (MONO_ADVANCE_EM + tracking_em)
}

fn sans_advance_em(c: char) -> f64 {
    match c {
        'i' | 'l' | 'j' | '.' | ',' | '\'' | '|' | ':' | ';' | '!' => 0.28,
        ' ' | 'I' | 'f' | 't' | 'r' | '(' | ')' | '-' => 0.36,
        'm' | 'w' => 0.86,
        'M' | 'W' => 0.9,
        c if c.is_ascii_uppercase() => 0.68,
        c if c.is_ascii_digit() => 0.6,
        _ => 0.58,
    }
}

/// Splits `text` at the space nearest its middle, for names too wide for one line.
pub fn split_in_two(text: &str) -> Option<(String, String)> {
    let middle = text.len() / 2;
    let at = text
        .char_indices()
        .filter(|(_, c)| *c == ' ')
        .map(|(i, _)| i)
        .min_by_key(|i| i.abs_diff(middle))?;
    Some((text[..at].to_string(), text[at + 1..].to_string()))
}

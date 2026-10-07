//! Pest parser that rewrites Pico-8 dialect in place.
//!
//! Edits are byte ranges into the original source. An outer rewrite (a shorthand
//! `if` that contains `+=`) is built from the inner edits, then the inner ranges
//! are skipped when the edits are applied.
use pest::Parser;
use pest::error::LineColLocation;
use pest_derive::Parser;
use std::borrow::Cow;
use std::fmt;

/// A Pico-8 snippet the grammar rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line of the failure.
    pub line: usize,
    /// 1-based column of the failure.
    pub column: usize,
    message: String,
}

impl ParseError {
    fn from_pest(err: pest::error::Error<Rule>) -> Self {
        let (line, column) = match err.line_col {
            LineColLocation::Pos((line, column)) | LineColLocation::Span((line, column), _) => {
                (line, column)
            }
        };
        Self {
            line,
            column,
            message: err.to_string(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Parser)]
#[grammar = "src/p8lua.pest"]
struct P8LuaParser;

struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

/// Parse `src` and rewrite Pico-8 dialect, preserving everything else.
///
/// `Err` is the parse failure. `Ok` is the rewritten source, borrowed when
/// nothing changed.
pub fn try_patch(src: &str) -> Result<Cow<'_, str>, ParseError> {
    let mut pairs = P8LuaParser::parse(Rule::chunk, src).map_err(ParseError::from_pest)?;
    let chunk = pairs.next().ok_or_else(|| ParseError {
        line: 1,
        column: 1,
        message: "empty parse".to_string(),
    })?;
    let mut edits = gap_comment_edits(src, &chunk);
    collect(chunk, src, &mut edits);
    if edits.is_empty() {
        return Ok(Cow::Borrowed(src));
    }
    Ok(Cow::Owned(materialize(src, 0, src.len(), &edits)))
}

fn collect(pair: pest::iterators::Pair<'_, Rule>, src: &str, edits: &mut Vec<Edit>) {
    let rule = pair.as_rule();
    let span = pair.as_span();
    let start = span.start();
    let end = span.end();
    let children: Vec<_> = pair.into_inner().collect();
    for child in &children {
        collect(child.clone(), src, edits);
    }

    match rule {
        Rule::compound_assign => {
            let var = child_span(&children, Rule::var);
            let op = child_str(&children, Rule::compound_op);
            let exp = child_span(&children, Rule::expr);
            if let (Some((vs, ve)), Some(op), Some((es, ee))) = (var, op, exp) {
                // Implicit skip sticks to the span when a later repeat or optional fails.
                let ve = code_end(src, vs, ve);
                let ee = code_end(src, es, ee);
                let end = code_end(src, start, end);
                let bin = &op[..op.len() - 1];
                let var_txt = materialize(src, vs, ve, edits);
                let exp_txt = materialize(src, es, ee, edits);
                edits.push(Edit {
                    start,
                    end,
                    replacement: format!("{var_txt} = {var_txt} {bin} ({exp_txt})"),
                });
            }
        }
        Rule::shorthand_if => {
            if let Some(repl) = shorthand_replacement(src, &children, "if", "then", edits) {
                edits.push(Edit {
                    start,
                    end: code_end(src, start, end),
                    replacement: repl,
                });
            }
        }
        Rule::shorthand_while => {
            if let Some(repl) = shorthand_replacement(src, &children, "while", "do", edits) {
                edits.push(Edit {
                    start,
                    end: code_end(src, start, end),
                    replacement: repl,
                });
            }
        }
        Rule::print_stmt if start < end && src.as_bytes().get(start) == Some(&b'?') => {
            let end = code_end(src, start, end);
            let args_start = trim_ws_start(src, start + 1, end);
            let args = materialize(src, args_start, end, edits);
            edits.push(Edit {
                start,
                end,
                replacement: format!("print({args})"),
            });
        }
        Rule::cmp_op if span.as_str() == "!=" => {
            edits.push(Edit {
                start,
                end,
                replacement: "~=".to_string(),
            });
        }
        Rule::binary => {
            edits.push(Edit {
                start,
                end,
                replacement: binary_to_hex(span.as_str()),
            });
        }
        Rule::button => {
            edits.push(Edit {
                start,
                end,
                replacement: button_digit(span.as_str()),
            });
        }
        _ => {}
    }
}

/// `if cond then body end` or `while cond do body end`, including a same-line else.
fn shorthand_replacement(
    src: &str,
    children: &[pest::iterators::Pair<'_, Rule>],
    keyword: &str,
    opener: &str,
    edits: &[Edit],
) -> Option<String> {
    let (cs, ce) = child_span(children, Rule::expr)?;
    let cond = materialize(src, cs, code_end(src, cs, ce), edits);
    let mut bodies = children
        .iter()
        .filter(|c| c.as_rule() == Rule::shorthand_body);
    let body = bodies.next()?;
    let body_txt = materialize_trimmed(src, body.as_span().start(), body.as_span().end(), edits);
    if let Some(else_body) = bodies.next() {
        let else_txt = materialize_trimmed(
            src,
            else_body.as_span().start(),
            else_body.as_span().end(),
            edits,
        );
        Some(format!(
            "{keyword} {cond} {opener} {body_txt} else {else_txt} end"
        ))
    } else {
        Some(format!("{keyword} {cond} {opener} {body_txt} end"))
    }
}

fn child_span(children: &[pest::iterators::Pair<'_, Rule>], rule: Rule) -> Option<(usize, usize)> {
    children.iter().find(|c| c.as_rule() == rule).map(|c| {
        let s = c.as_span();
        (s.start(), s.end())
    })
}

fn child_str<'a>(children: &'a [pest::iterators::Pair<'_, Rule>], rule: Rule) -> Option<&'a str> {
    children
        .iter()
        .find(|c| c.as_rule() == rule)
        .map(|c| c.as_str())
}

fn materialize_trimmed(src: &str, start: usize, end: usize, edits: &[Edit]) -> String {
    materialize(src, start, code_end(src, start, end), edits)
}

fn trim_ws_start(src: &str, mut start: usize, end: usize) -> usize {
    let bytes = src.as_bytes();
    while start < end && is_ws(bytes[start]) {
        start += 1;
    }
    start
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// End of the real tokens in `start..end`.
///
/// Pest commits implicit whitespace and comments before a repeat or optional that
/// then fails, so a span can include a trailing space or a trailing comment.
/// Those belong outside the rewrite (`x += 1 -- hi` must not put the comment
/// inside the parentheses).
fn code_end(src: &str, start: usize, end: usize) -> usize {
    let bytes = src.as_bytes();
    let mut i = start;
    let mut last_code = start;
    while i < end {
        if is_ws(bytes[i]) {
            i += 1;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            i = skip_short_string(bytes, i, end);
            last_code = i;
            continue;
        }
        if bytes[i] == b'[' {
            if let Some(j) = skip_long_brackets(bytes, i, end) {
                i = j;
                last_code = i;
                continue;
            }
        }
        if i + 1 < end
            && ((bytes[i] == b'/' && bytes[i + 1] == b'/')
                || (bytes[i] == b'-' && bytes[i + 1] == b'-'))
        {
            let comment_end = if bytes[i] == b'/' {
                skip_line(bytes, i + 2).min(end)
            } else {
                skip_dash_comment(bytes, i).min(end)
            };
            if comment_end <= i {
                i += 1;
                last_code = i;
                continue;
            }
            if bytes[comment_end..end].iter().copied().all(is_ws) {
                return last_code;
            }
            i = comment_end;
            continue;
        }
        i += 1;
        last_code = i;
    }
    last_code
}

fn skip_short_string(bytes: &[u8], i: usize, limit: usize) -> usize {
    let quote = bytes[i];
    let mut j = i + 1;
    while j < limit {
        if bytes[j] == b'\\' {
            j += 2;
            if j > limit {
                return limit;
            }
            continue;
        }
        if bytes[j] == quote {
            return j + 1;
        }
        if bytes[j] == b'\n' || bytes[j] == b'\r' {
            return j;
        }
        j += 1;
    }
    limit
}

fn skip_long_brackets(bytes: &[u8], i: usize, limit: usize) -> Option<usize> {
    if i >= limit || bytes[i] != b'[' {
        return None;
    }
    let mut j = i + 1;
    while j < limit && bytes[j] == b'=' {
        j += 1;
    }
    if j >= limit || bytes[j] != b'[' {
        return None;
    }
    let eqs = j - (i + 1);
    j += 1;
    while j < limit {
        if bytes[j] == b']' {
            let close = j + 1 + eqs;
            if close < limit
                && bytes[close] == b']'
                && bytes[j + 1..close].iter().all(|b| *b == b'=')
            {
                return Some(close + 1);
            }
        }
        j += 1;
    }
    Some(limit)
}

/// Apply `edits` that sit inside `start..end`. A larger edit hides the ones it contains.
fn materialize(src: &str, start: usize, end: usize, edits: &[Edit]) -> String {
    let mut relevant: Vec<&Edit> = edits
        .iter()
        .filter(|e| e.start >= start && e.end <= end && e.start < e.end)
        .collect();
    relevant.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));

    let mut out = String::new();
    let mut cursor = start;
    for edit in relevant {
        if edit.start < cursor {
            continue;
        }
        out.push_str(&src[cursor..edit.start]);
        out.push_str(&edit.replacement);
        cursor = edit.end;
    }
    out.push_str(&src[cursor..end]);
    out
}

/// `//` comments live in the gaps between tokens (whitespace and comments).
/// Strings are tokens, so a `//` inside one is not a gap.
fn gap_comment_edits(src: &str, chunk: &pest::iterators::Pair<'_, Rule>) -> Vec<Edit> {
    let mut leaves = Vec::new();
    collect_leaves(chunk.clone(), &mut leaves);
    leaves.sort_by_key(|span| span.0);
    leaves.dedup();

    let mut edits = Vec::new();
    let mut cursor = 0;
    for (start, end) in leaves {
        if start > cursor {
            rewrite_comment_gap(&src[cursor..start], cursor, &mut edits);
        }
        cursor = cursor.max(end);
    }
    if cursor < src.len() {
        rewrite_comment_gap(&src[cursor..], cursor, &mut edits);
    }
    edits
}

fn collect_leaves(pair: pest::iterators::Pair<'_, Rule>, leaves: &mut Vec<(usize, usize)>) {
    let span = pair.as_span();
    // Keep string and number text intact so `//` and `!=` inside them are not gaps.
    if matches!(
        pair.as_rule(),
        Rule::string
            | Rule::number
            | Rule::name
            | Rule::button
            | Rule::include
            | Rule::long_brackets
            | Rule::short_string
    ) {
        if span.start() < span.end() {
            leaves.push((span.start(), span.end()));
        }
        return;
    }
    let children: Vec<_> = pair.into_inner().collect();
    if children.is_empty() {
        if span.start() < span.end() {
            leaves.push((span.start(), span.end()));
        }
        return;
    }
    for child in children {
        collect_leaves(child, leaves);
    }
}

fn rewrite_comment_gap(gap: &str, base: usize, edits: &mut Vec<Edit>) {
    let bytes = gap.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'-' && bytes[i + 1] == b'-' {
            i = skip_dash_comment(bytes, i);
            continue;
        }
        if bytes[i] == b'/' && bytes[i + 1] == b'/' {
            edits.push(Edit {
                start: base + i,
                end: base + i + 2,
                replacement: "--".to_string(),
            });
            i = skip_line(bytes, i + 2);
            continue;
        }
        i += 1;
    }
}

fn skip_dash_comment(bytes: &[u8], i: usize) -> usize {
    // `--[` equals `[` opens a long comment. Anything else is a line comment,
    // including `--[not a long comment`.
    let after = i + 2;
    if bytes.get(after) == Some(&b'[') {
        let mut j = after + 1;
        let eq_start = j;
        while bytes.get(j) == Some(&b'=') {
            j += 1;
        }
        if bytes.get(j) == Some(&b'[') {
            let eqs = j - eq_start;
            j += 1;
            while j < bytes.len() {
                if bytes[j] == b']'
                    && bytes.get(j + 1 + eqs) == Some(&b']')
                    && bytes[j + 1..j + 1 + eqs].iter().all(|b| *b == b'=')
                {
                    return j + 2 + eqs;
                }
                j += 1;
            }
            return bytes.len();
        }
    }
    skip_line(bytes, after)
}

fn skip_line(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
        i += 1;
    }
    i
}

fn binary_to_hex(text: &str) -> String {
    let rest = &text[2..];
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((int_part, frac)) => (int_part, Some(frac)),
        None => (rest, None),
    };
    let int_hex = bits_to_hex(int_part, true);
    match frac_part {
        Some(frac) if !frac.is_empty() => format!("0x{int_hex}.{}", bits_to_hex(frac, false)),
        _ => format!("0x{int_hex}"),
    }
}

fn bits_to_hex(bits: &str, pad_left: bool) -> String {
    if bits.is_empty() {
        return "0".into();
    }
    let mut padded = bits.to_string();
    let rem = padded.len() % 4;
    if rem != 0 {
        let pad = "0".repeat(4 - rem);
        if pad_left {
            padded.insert_str(0, &pad);
        } else {
            padded.push_str(&pad);
        }
    }
    let mut hex = String::new();
    for chunk in padded.as_bytes().chunks(4) {
        let value = chunk
            .iter()
            .fold(0u32, |acc, bit| (acc << 1) | u32::from(*bit == b'1'));
        hex.push(char::from_digit(value, 16).unwrap());
    }
    if pad_left {
        let trimmed = hex.trim_start_matches('0');
        if trimmed.is_empty() {
            "0".into()
        } else {
            trimmed.into()
        }
    } else {
        hex
    }
}

/// Shift+letter glyphs Pico-8 predefines as numbers.
///
/// Buttons are 0–5. The other twenty are `fillp` patterns: the integer is the
/// signed 16-bit pattern, and `.5` is the transparency bit.
fn button_digit(text: &str) -> String {
    match text.trim_end_matches('️') {
        "⬅" => "0",
        "➡" => "1",
        "⬆" => "2",
        "⬇" => "3",
        "🅾" => "4",
        "❎" => "5",
        "█" => "0.5",
        "▒" => "23130.5",
        "🐱" => "20767.5",
        "░" => "32125.5",
        "✽" => "-18402.5",
        "●" => "-1632.5",
        "♥" => "20927.5",
        "☉" => "-19008.5",
        "웃" => "-26208.5",
        "⌂" => "-20192.5",
        "😐" => "-24351.5",
        "♪" => "-25792.5",
        "◆" => "-20032.5",
        "…" => "-2560.5",
        "★" => "-20128.5",
        "⧗" => "6943.5",
        "ˇ" => "-2624.5",
        "∧" => "31455.5",
        "▤" => "3855.5",
        "▥" => "21845.5",
        other => other,
    }
    .to_string()
}

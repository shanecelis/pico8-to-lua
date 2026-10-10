//! Pest parser that rewrites Pico-8 dialect in place.
//!
//! Edits are byte ranges into the original source. An outer rewrite (a shorthand
//! `if` that contains `+=`) is built from the inner edits, then the inner ranges
//! are skipped when the edits are applied.
use pest::Parser;
use pest::error::LineColLocation;
use pest_derive::Parser;
use std::borrow::Cow;

/// Why a snippet failed to parse.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    /// The grammar rejected the snippet.
    #[error(transparent)]
    Pest(#[from] pest::error::Error<Rule>),
}

impl ParseError {
    /// 1-based line and column of the failure.
    pub fn line_column(&self) -> (u32, u32) {
        let ParseError::Pest(err) = self;
        let (line, column) = match err.line_col {
            LineColLocation::Pos((line, column))
            | LineColLocation::Span((line, column), _) => (line, column),
        };
        (line as u32, column as u32)
    }
}

#[derive(Parser)]
#[grammar = "src/p8lua.pest"]
struct P8LuaParser;

struct Edit<'a> {
    start: usize,
    end: usize,
    replacement: Cow<'a, str>,
}

/// Parse `src` and rewrite Pico-8 dialect, preserving everything else.
///
/// `Err` is the parse failure. `Ok` is either the owned rewritten source, or
/// borrowed argument that was given.
pub fn try_patch(src: &str) -> Result<Cow<'_, str>, ParseError> {
    let mut pairs = P8LuaParser::parse(Rule::chunk, src).map_err(ParseError::from)?;
    let Some(chunk) = pairs.next() else {
        return Ok(Cow::Borrowed(src));
    };
    let mut edits = gap_comment_edits(src, &chunk);
    collect(chunk, src, &mut edits);
    if edits.is_empty() {
        return Ok(Cow::Borrowed(src));
    }
    Ok(Cow::Owned(materialize(src, 0, src.len(), &edits)))
}

pub(crate) struct Include<'a> {
    pub start: usize,
    pub end: usize,
    pub path: &'a str,
}

/// `#include` directives whose `#` is the first non-space character on the line.
///
/// A source that does not parse has no directives.
pub(crate) fn includes(src: &str) -> Result<Vec<Include<'_>>, ParseError> {
    let mut pairs = P8LuaParser::parse(Rule::chunk, src).map_err(ParseError::from)?;
    let Some(chunk) = pairs.next() else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    walk_includes(chunk, src, &mut found);
    Ok(found)
}

fn walk_includes<'a>(
    pair: pest::iterators::Pair<'a, Rule>,
    src: &'a str,
    out: &mut Vec<Include<'a>>,
) {
    if pair.as_rule() == Rule::include {
        let span = pair.as_span();
        if let Some(path) = pair
            .into_inner()
            .find(|child| child.as_rule() == Rule::include_path)
        {
            let path = path.as_span();
            if directive_at_line_start(src, span.start()) {
                out.push(Include {
                    start: span.start(),
                    end: path.end(),
                    path: path.as_str(),
                });
            }
        }
        return;
    }
    for child in pair.into_inner() {
        walk_includes(child, src, out);
    }
}

fn directive_at_line_start(src: &str, hash: usize) -> bool {
    let bytes = src.as_bytes();
    let mut i = hash;
    while i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
        i -= 1;
    }
    i == 0 || matches!(bytes[i - 1], b'\n' | b'\r')
}

fn collect<'a>(pair: pest::iterators::Pair<'a, Rule>, src: &'a str, edits: &mut Vec<Edit<'a>>) {
    let rule = pair.as_rule();
    let span = pair.as_span();
    let start = span.start();
    let end = span.end();
    let text = span.as_str();

    // Only the rewrites that read child spans keep the children. Every other
    // node is walked without allocating a child list.
    let call_shift = rule == Rule::shift_expr
        && (text.contains(">>>") || text.contains("<<>") || text.contains(">><"));
    if matches!(
        rule,
        Rule::compound_assign | Rule::shorthand_if | Rule::shorthand_while | Rule::peek
    ) || call_shift
    {
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
                    let var_txt = materialize(src, vs, ve, edits);
                    let exp_txt = materialize(src, es, ee, edits);
                    edits.push(Edit {
                        start,
                        end,
                        replacement: Cow::Owned(compound_replacement(&var_txt, op, &exp_txt)),
                    });
                }
            }
            Rule::peek => {
                if let Some(repl) = peek_replacement(src, &children, edits) {
                    edits.push(Edit {
                        start,
                        end: code_end(src, start, end),
                        replacement: Cow::Owned(repl),
                    });
                }
            }
            Rule::shift_expr => {
                if let Some(repl) = shift_replacement(src, &children, edits) {
                    edits.push(Edit {
                        start,
                        end: code_end(src, start, end),
                        replacement: Cow::Owned(repl),
                    });
                }
            }
            Rule::shorthand_if => {
                if let Some(repl) = shorthand_replacement(src, &children, "if", "then", edits) {
                    edits.push(Edit {
                        start,
                        end: code_end(src, start, end),
                        replacement: Cow::Owned(repl),
                    });
                }
            }
            Rule::shorthand_while => {
                if let Some(repl) = shorthand_replacement(src, &children, "while", "do", edits) {
                    edits.push(Edit {
                        start,
                        end: code_end(src, start, end),
                        replacement: Cow::Owned(repl),
                    });
                }
            }
            _ => {}
        }
        return;
    }

    for child in pair.into_inner() {
        collect(child, src, edits);
    }

    match rule {
        Rule::print_stmt if start < end && src.as_bytes().get(start) == Some(&b'?') => {
            let end = code_end(src, start, end);
            let args_start = trim_ws_start(src, start + 1, end);
            let args = materialize(src, args_start, end, edits);
            edits.push(Edit {
                start,
                end,
                replacement: Cow::Owned(format!("print({args})")),
            });
        }
        Rule::if_then if text == "do" => {
            edits.push(Edit {
                start,
                end,
                replacement: Cow::Borrowed("then"),
            });
        }
        Rule::cmp_op if text == "!=" => {
            edits.push(Edit {
                start,
                end,
                replacement: Cow::Borrowed("~="),
            });
        }
        Rule::xor_op if text == "^^" => {
            edits.push(Edit {
                start,
                end,
                replacement: Cow::Borrowed("~"),
            });
        }
        Rule::mul_op if text == "\\" => {
            edits.push(Edit {
                start,
                end,
                replacement: Cow::Borrowed("//"),
            });
        }
        Rule::binary => {
            edits.push(Edit {
                start,
                end,
                replacement: Cow::Owned(binary_to_hex(text)),
            });
        }
        Rule::button => {
            edits.push(Edit {
                start,
                end,
                replacement: button_digit(text),
            });
        }
        _ => {}
    }
}

/// `var \= exp` and the other Pico-8 assignment operators, in Lua.
fn compound_replacement(var_txt: &str, op: &str, exp_txt: &str) -> String {
    let bin = &op[..op.len() - 1];
    match bin {
        "\\" => format!("{var_txt} = {var_txt} // ({exp_txt})"),
        "^^" => format!("{var_txt} = {var_txt} ~ ({exp_txt})"),
        ">>>" => format!("{var_txt} = lshr({var_txt}, ({exp_txt}))"),
        "<<>" => format!("{var_txt} = rotl({var_txt}, ({exp_txt}))"),
        ">><" => format!("{var_txt} = rotr({var_txt}, ({exp_txt}))"),
        _ => format!("{var_txt} = {var_txt} {bin} ({exp_txt})"),
    }
}

fn peek_replacement(
    src: &str,
    children: &[pest::iterators::Pair<'_, Rule>],
    edits: &[Edit<'_>],
) -> Option<String> {
    let op = child_str(children, Rule::peek_op)?;
    let (start, end) = child_span(children, Rule::peek_operand)?;
    let arg = materialize(src, start, code_end(src, start, end), edits);
    let name = match op {
        "@" => "peek",
        "%" => "peek2",
        "$" => "peek4",
        _ => return None,
    };
    Some(format!("{name}({arg})"))
}

/// `>>>` `<<>` `>><` become calls. `<<` and `>>` stay, including beside a call:
/// `a >>> b << c` is `lshr(a, b) << c`.
fn shift_replacement(
    src: &str,
    children: &[pest::iterators::Pair<'_, Rule>],
    edits: &[Edit<'_>],
) -> Option<String> {
    let atom = children.iter().find(|c| c.as_rule() == Rule::shift_atom)?;
    let mut acc: Option<String> = None;
    let mut prev_end = atom.as_span().end();
    let expr_start = atom.as_span().start();

    for step in children.iter().filter(|c| c.as_rule() == Rule::shift_step) {
        let inner: Vec<_> = step.clone().into_inner().collect();
        let op = child_str(&inner, Rule::shift_op)?;
        let (right_start, right_end) = child_span(&inner, Rule::shift_atom)?;
        let right_end = code_end(src, right_start, right_end);
        let right_txt = materialize(src, right_start, right_end, edits);
        let op_pair = inner.iter().find(|c| c.as_rule() == Rule::shift_op)?;
        let op_start = op_pair.as_span().start();
        let op_end = op_pair.as_span().end();
        if let Some(func) = shift_func(op) {
            let left_txt = match &acc {
                Some(left) => left.clone(),
                None => materialize(src, expr_start, code_end(src, expr_start, prev_end), edits),
            };
            acc = Some(format!("{func}({left_txt}, {right_txt})"));
        } else if let Some(left) = acc.as_deref() {
            let before_op = &src[prev_end..op_start];
            let after_op = &src[op_end..right_start];
            acc = Some(format!("{left}{before_op}{op}{after_op}{right_txt}"));
        }
        prev_end = step.as_span().end();
    }
    acc
}

fn shift_func(op: &str) -> Option<&'static str> {
    match op {
        ">>>" => Some("lshr"),
        "<<>" => Some("rotl"),
        ">><" => Some("rotr"),
        _ => None,
    }
}

/// `if cond then body end` or `while cond do body end`, including a same-line else.
fn shorthand_replacement(
    src: &str,
    children: &[pest::iterators::Pair<'_, Rule>],
    keyword: &str,
    opener: &str,
    edits: &[Edit<'_>],
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

fn materialize_trimmed(src: &str, start: usize, end: usize, edits: &[Edit<'_>]) -> String {
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
fn materialize(src: &str, start: usize, end: usize, edits: &[Edit<'_>]) -> String {
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
fn gap_comment_edits<'a>(src: &'a str, chunk: &pest::iterators::Pair<'_, Rule>) -> Vec<Edit<'a>> {
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
    let start = span.start();
    let end = span.end();
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
        if start < end {
            leaves.push((start, end));
        }
        return;
    }
    let mut saw_child = false;
    for child in pair.into_inner() {
        saw_child = true;
        collect_leaves(child, leaves);
    }
    if !saw_child && start < end {
        leaves.push((start, end));
    }
}

fn rewrite_comment_gap<'a>(gap: &'a str, base: usize, edits: &mut Vec<Edit<'a>>) {
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
                replacement: Cow::Borrowed("--"),
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
fn button_digit(text: &str) -> Cow<'static, str> {
    Cow::Borrowed(match text.trim_end_matches('️') {
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
        _ => return Cow::Owned(text.trim_end_matches('️').to_string()),
    })
}

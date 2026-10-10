#![doc(html_root_url = "https://docs.rs/pico8-to-lua/0.2.0")]
#![doc = include_str!("../README.md")]
/// Copyright (c) 2015 Jez Kabanov <thesleepless@gmail.com>
/// Modified (c) 2019 Ben Wiley <therealbenwiley@gmail.com>
/// Modified (c) 2025 Shane Celis <shane.celis@gmail.com>
///
/// Original 2015 code from
/// [here](https://github.com/picolove/picolove/blob/d5a65fd6dd322532d90ea612893a00a28096804a/main.lua#L820).
///
/// Modified 2019 code from
/// [here](https://github.com/benwiley4000/pico8-to-lua/blob/master/pico8-to-lua.lua).
///
/// Licensed under the Zlib license.
use std::borrow::Cow;

mod parse;

pub use parse::Error;

/// Resolve the Pico-8 "#include path.p8" statements with possible errors.
///
/// If there are substitution errors, the first error will be returned. A source
/// that does not parse is returned unchanged.
pub fn try_patch_includes<'h, E: std::error::Error>(
    lua: impl Into<Cow<'h, str>>,
    mut resolve: impl FnMut(&str) -> Result<String, E>,
) -> Result<Cow<'h, str>, E> {
    let lua = lua.into();
    let includes = parse::includes(lua.as_ref()).unwrap_or_default();
    if includes.is_empty() {
        return Ok(lua);
    }
    let mut error = None;
    let mut edits = Vec::with_capacity(includes.len());
    for include in includes {
        match resolve(include.path) {
            Ok(s) => edits.push((include.start, include.end, s)),
            Err(e) => {
                // This is kind of pointless since the user will never get
                // access to the string. I'm leaving here incase the results
                // change to make it relevant later.
                let result = format!("error(\"failed to include {:?}: {}\")", include.path, e);
                if error.is_none() {
                    error = Some(e);
                }
                edits.push((include.start, include.end, result));
            }
        }
    }
    let patched = Cow::Owned(splice(lua.as_ref(), &edits));
    match error {
        Some(err) => Err(err),
        None => Ok(patched),
    }
}

/// Returns true if the patch_output was patched by testing whether it is
/// `Cow::Owned`; a `Cow::Borrowed` implies it was not patched.
#[allow(clippy::ptr_arg)]
pub fn was_patched(patch_output: &Cow<'_, str>) -> bool {
    match patch_output {
        Cow::Owned(_) => true,
        Cow::Borrowed(_) => false,
    }
}

/// Resolve the Pico-8 "#include path.p8" statements without possible error.
///
/// A source that does not parse is returned unchanged.
pub fn patch_includes<'h, 'r>(
    lua: impl Into<Cow<'h, str>>,
    mut resolve: impl FnMut(&str) -> Cow<'r, str>,
) -> Cow<'h, str>
where
    'r: 'h,
{
    let lua = lua.into();
    let includes = parse::includes(lua.as_ref()).unwrap_or_default();
    if includes.is_empty() {
        return lua;
    }
    let edits: Vec<_> = includes
        .iter()
        .map(|include| {
            (
                include.start,
                include.end,
                resolve(include.path).into_owned(),
            )
        })
        .collect();
    Cow::Owned(splice(lua.as_ref(), &edits))
}

fn splice(src: &str, edits: &[(usize, usize, String)]) -> String {
    // let capacity = src.len();
    let mut capacity = 0;
    let mut last = 0;
    for (start, end, replacement) in edits {
        capacity += start - last + replacement.len();
        last = *end;
    }
    capacity += src.len() - last;
    let mut out = String::with_capacity(capacity);
    last = 0;
    for (start, end, replacement) in edits {
        out.push_str(&src[last..*start]);
        out.push_str(replacement);
        last = *end;
    }
    out.push_str(&src[last..]);
    // assert!(capacity >= out.len());
    out
}

/// Return each path from the Pico-8 "#include path.p8" statements.
///
/// This function is not strictly necessary if one can read the includes
/// synchronously using [patch_includes] or [try_patch_includes]. However, in an
/// asynchronous IO context, it is often necessary to read in the contents
/// before patching the includes.
///
/// A source that does not parse yields no paths. An include written inside a
/// string or a comment is not a directive.
pub fn find_includes(lua: &str) -> impl Iterator<Item = &str> {
    parse::includes(lua)
        .unwrap_or_default()
        .into_iter()
        .map(|include| include.path)
}

/// Given a string with the Pico-8 dialect of Lua, it will convert that code to
/// plain Lua.
///
/// This function will not handle "#include path.p8" statements. It is
/// recommended to use [patch_includes] before this function since if those
/// inclusions may use the Pico-8 dialect.
///
/// Parses Pico-8 Lua and rewrites the dialect in place. A snippet that does not
/// parse returns [`Error`].
pub fn patch_lua<'h>(lua: impl Into<Cow<'h, str>>) -> Result<Cow<'h, str>, Error> {
    let lua = lua.into();
    match parse::try_patch(lua.as_ref())? {
        Cow::Borrowed(_) => Ok(lua),
        Cow::Owned(patched) => Ok(Cow::Owned(patched)),
    }
}

/// Whether the pest grammar accepts `src`. The benchmark uses this so a parse
/// failure is not timed as a successful rewrite.
#[doc(hidden)]
pub fn bench_parsed(src: &str) -> bool {
    parse::try_patch(src).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    fn ok(lua: &str) -> Cow<'_, str> {
        patch_lua(lua).unwrap_or_else(|err| panic!("{err}"))
    }

    /// P8SCII 16–29 and 127. Icons and Japanese punctuation, not identifiers.
    const P8_PUNCT: &str = "\
        ▮■□⁙⁘‖◀▶\
        「」¥•、。○";

    /// P8SCII 30–31 and 128–255, the characters Pico-8 allows in a name.
    const P8_IDENT_CHARS: &str = "\
        ゛゜\
        █▒🐱⬇░✽●♥\
        ☉웃⌂⬅😐♪🅾◆\
        …➡★⧗⬆ˇ∧❎\
        ▤▥\
        あいうえお\
        かきくけこ\
        さしすせそ\
        たちつてと\
        なにぬねの\
        はひふへほ\
        まみむめも\
        やゆよ\
        らりるれろ\
        わをん\
        っゃゅょ\
        アイウエオ\
        カキクケコ\
        サシスセソ\
        タチツテト\
        ナニヌネノ\
        ハヒフヘホ\
        マミムメモ\
        ヤユヨ\
        ラリルレロ\
        ワヲン\
        ッャュョ\
        ◜◝";

    /// Unicode Pico-8 writes for C0 controls other than tab, newline, and CR.
    /// The wiki lists these in the P8SCII control-code table.
    const P8_CONTROLS: &str = "\
        ¹²³⁴⁵⁶⁷⁸\
        ᵇᶜᵉᶠ";

    #[test]
    fn test_not_equal_replacement() {
        let lua = "if a != b then print(a) end";
        let patched = ok(lua);
        assert!(patched.contains("a ~= b"));
    }

    #[test]
    fn test_comment_replacement() {
        let lua = "// this is a comment\nprint('hello')";
        let patched = ok(lua);
        assert!(patched.contains("-- this is a comment"));
    }

    #[test]
    fn test_shorthand_if_rewrite() {
        let lua = "if (not b) i = 1\n";
        let expected = "if not b then i = 1 end\n";
        let patched = ok(lua);
        assert_eq!(patched, expected);
    }

    #[test]
    fn test_shorthand_if_rewrite_comment() {
        let lua = "if (not b) i = 1 // hi\n";
        let expected = "if not b then i = 1 end -- hi\n";
        let patched = ok(lua);
        assert_eq!(patched, expected);
    }

    #[test]
    fn test_shorthand_if_compound_with_dash_comment() {
        // jelpi.p8: a trailing `--` on a shorthand if is a comment, not minus.
        let lua = "if (j==2) hx -=.5 --hy-=.7\n";
        let expected = "if j==2 then hx = hx - (.5) end --hy-=.7\n";
        assert_eq!(ok(lua), expected);
    }

    #[test]
    fn test_assign_field_of_call() {
        // celeste.p8: store a field on the table the call returns.
        let lua = "init_object(platform,tx*8,ty*8).dir=-1\n";
        assert_eq!(ok(lua), lua);
    }

    #[test]
    fn test_if_do_is_then() {
        // caveofcards.p8: `do` closes the condition where Lua wants `then`.
        let lua = "if rt == hp1blocks[1] or rt == hp1blocks[2] do\nend\n";
        let expected = "if rt == hp1blocks[1] or rt == hp1blocks[2] then\nend\n";
        assert_eq!(ok(lua), expected);
        assert_eq!(ok("if (c) do x() end\n"), "if c then do x() end end\n");
    }

    #[test]
    fn test_shorthand_if_rewrite_and() {
        let lua = "if (not b and not c) i = 1\n";
        let expected = "if not b and not c then i = 1 end\n";
        let patched = ok(lua);
        assert_eq!(patched, expected);
    }

    #[test]
    fn test_assignment_operator_rewrite() {
        let lua = "x += 1";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "x = x + (1)");
    }

    #[test]
    fn test_question_print_conversion0() {
        let lua = "?x";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "print(x)");
    }

    #[test]
    fn test_question_print_conversion() {
        let lua = "?x + y";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "print(x + y)");
    }

    #[test]
    fn test_binary_literal_conversion_integer() {
        let lua = "a = 0b1010";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "a = 0xa");
    }

    #[test]
    fn test_binary_literal_conversion_fractional() {
        let lua = "a = 0b1010.1";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "a = 0xa.8");
    }

    #[test]
    fn test_mixed_transforms() {
        let lua = r#"
        // comment
        if (a != b) x += 1
        ?x
        "#;
        let patched = ok(lua);
        assert!(patched.contains("-- comment"), "{}", patched);
        assert!(
            patched.contains("if a ~= b then x = x + (1) end"),
            "{}",
            patched
        );
        assert!(patched.contains("print(x)"), "{}", patched);
    }

    #[test]
    fn test_no_change_no_allocation() {
        let lua = "x = 1";
        let patched = ok(lua);
        // assert!(patched.is_borrowed());
        assert!(match patched {
            Cow::Owned(_) => false,
            Cow::Borrowed(_) => true,
        });
    }

    #[test]
    fn test_change_requires_allocation() {
        let lua = "x += 1";
        let patched = ok(lua);
        // assert!(patched.is_owned());
        assert!(match patched {
            Cow::Owned(_) => true,
            Cow::Borrowed(_) => false,
        });
    }

    #[test]
    fn test_includes() {
        let lua = r#"
        #include blah.p8
        "#;
        let patched = patch_includes(lua, |path| format!("-- INCLUDE {}", path).into());
        assert!(patched.contains("-- INCLUDE blah.p8"), "{}", &patched);
    }

    #[test]
    fn test_bad_comment() {
        let lua = "--==configurations==--";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "--==configurations==--");
    }

    #[test]
    fn test_bad_if() {
        let lua =
            "if (ord(tb.str[tb.i],tb.char)!=32) sfx(tb.voice) -- play the voice sound effect.";
        let patched = ok(lua);
        assert_eq!(
            patched.trim(),
            "if ord(tb.str[tb.i],tb.char)~=32 then sfx(tb.voice) end -- play the voice sound effect."
        );
    }

    #[test]
    fn test_bad_incr() {
        let lua = "tb.i+=1 -- increase the index, to display the next message on tb.str";
        let patched = ok(lua);
        assert_eq!(
            patched.trim(),
            "tb.i = tb.i + (1) -- increase the index, to display the next message on tb.str"
        );
    }

    #[test]
    fn test_button() {
        let lua = "if btnp(➡️) or btn(❎) then end";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "if btnp(1) or btn(5) then end");
    }

    #[test]
    fn test_button2() {
        let lua = "if btnp(❎) then end";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "if btnp(5) then end");
    }

    #[test]
    fn test_button3() {
        let lua = "if btnp(🅾) then end";
        let patched = ok(lua);
        assert_eq!(patched.trim(), "if btnp(4) then end");
    }

    #[test_case("⬅", "0" ; "left")]
    #[test_case("⬅️", "0" ; "left with variation selector")]
    #[test_case("➡", "1" ; "right")]
    #[test_case("➡️", "1" ; "right with variation selector")]
    #[test_case("⬆", "2" ; "up")]
    #[test_case("⬆️", "2" ; "up with variation selector")]
    #[test_case("⬇", "3" ; "down")]
    #[test_case("⬇️", "3" ; "down with variation selector")]
    #[test_case("🅾", "4" ; "o button")]
    #[test_case("🅾️", "4" ; "o button with variation selector")]
    #[test_case("❎", "5" ; "x button")]
    #[test_case("❎️", "5" ; "x button with variation selector")]
    fn button_glyph_in_code_and_string(glyph: &str, digit: &str) {
        let src =
            format!("btn({glyph})\nx = \"{glyph}\"\ny = '{glyph}'\nz = [[{glyph}]]\nw = {glyph}");
        let expected =
            format!("btn({digit})\nx = \"{glyph}\"\ny = '{glyph}'\nz = [[{glyph}]]\nw = {digit}");
        assert_eq!(ok(&src), expected);
    }

    // The other twenty Shift+letter glyphs. Pico-8 predefines them as fillp()
    // patterns; `.5` is the transparency bit. Strings keep the character.
    #[test_case("█", "0.5" ; "rectangle")]
    #[test_case("▒", "23130.5" ; "checkerboard")]
    #[test_case("🐱", "20767.5" ; "jelpi")]
    #[test_case("░", "32125.5" ; "dot pattern")]
    #[test_case("✽", "-18402.5" ; "throwing star")]
    #[test_case("●", "-1632.5" ; "ball")]
    #[test_case("♥", "20927.5" ; "heart")]
    #[test_case("☉", "-19008.5" ; "eye")]
    #[test_case("웃", "-26208.5" ; "man")]
    #[test_case("⌂", "-20192.5" ; "house")]
    #[test_case("😐", "-24351.5" ; "face")]
    #[test_case("♪", "-25792.5" ; "musical note")]
    #[test_case("◆", "-20032.5" ; "diamond")]
    #[test_case("…", "-2560.5" ; "ellipsis")]
    #[test_case("★", "-20128.5" ; "star")]
    #[test_case("⧗", "6943.5" ; "hourglass")]
    #[test_case("ˇ", "-2624.5" ; "birds")]
    #[test_case("∧", "31455.5" ; "sawtooth")]
    #[test_case("▤", "3855.5" ; "horiz lines")]
    #[test_case("▥", "21845.5" ; "vert lines")]
    fn fill_glyph_in_code_and_string(glyph: &str, number: &str) {
        let src =
            format!("fillp({glyph})\nx = \"{glyph}\"\ny = '{glyph}'\nz = [[{glyph}]]\nw = {glyph}");
        let expected = format!(
            "fillp({number})\nx = \"{glyph}\"\ny = '{glyph}'\nz = [[{glyph}]]\nw = {number}"
        );
        assert_eq!(ok(&src), expected);
    }

    #[test]
    fn glyph_used_as_a_name_stays() {
        // Pico-8's preprocessor accepts `♥.x += 1`. The glyph is a variable
        // there, not the fill-pattern constant.
        assert_eq!(ok("♥.x += 1"), "♥.x = ♥.x + (1)");
        assert_eq!(ok("♥ = 1\n❎ = 2"), "♥ = 1\n❎ = 2");
        assert_eq!(ok("❎foo = 1"), "❎foo = 1");
        assert_eq!(ok("if (♥) x+=1"), "if 20927.5 then x = x + (1) end");
        assert_eq!(ok("y = x^█"), "y = x^0.5");
        // U+2026 is the ellipsis glyph, not Lua's `...`.
        assert_eq!(ok("f(…)\nreturn ..."), "f(-2560.5)\nreturn ...");
    }

    #[test]
    fn p8scii_identifier_characters_stay() {
        // Dakuten, handakuten, kana, arcs, and the Shift glyphs as names.
        for ch in P8_IDENT_CHARS.chars() {
            let src = format!("local {ch} = 1\n{ch}.a += 2\n");
            let expected = format!("local {ch} = 1\n{ch}.a = {ch}.a + (2)\n");
            assert_eq!(ok(&src), expected, "U+{:04X}", u32::from(ch));
        }
    }

    #[test]
    fn p8scii_font_stays_in_strings_and_comments() {
        // Printable P8SCII outside ASCII, plus the unicode Pico-8 stores for
        // the C0 controls that are not tab, newline, or carriage return.
        let font = format!("{P8_PUNCT}{P8_IDENT_CHARS}{P8_CONTROLS}");
        let src = format!("x = \"{font}\"\ny = '{font}'\nz = [[{font}]]\n-- {font}\n// {font}\n");
        let expected =
            format!("x = \"{font}\"\ny = '{font}'\nz = [[{font}]]\n-- {font}\n-- {font}\n");
        assert_eq!(ok(&src), expected);
    }

    #[test]
    fn p8scii_punctuation_is_not_a_number() {
        // Codes 16-29 and the hollow circle are not the Shift-glyph constants.
        let err = patch_lua("x = ■").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("expected"), "{message}");
    }

    #[test]
    fn test_button_inside_if() {
        let lua = "if (btn(❎)) then\nend\nif (l%16==0 or btnp(❎)) then\nend";
        let patched = ok(lua);
        assert_eq!(
            patched.trim(),
            "if (btn(5)) then\nend\nif (l%16==0 or btnp(5)) then\nend"
        );
    }

    fn assert_patch(unpatched: &str, expected_patched: &str) {
        let patched = ok(unpatched);
        assert_eq!(patched, expected_patched);
    }

    #[test]
    fn test_cardboard_toad0() {
        assert_patch(
            "if (o.color) setmetatable(o.color, { __index = (message_instance or message).color })",
            "if o.color then setmetatable(o.color, { __index = (message_instance or message).color }) end",
        );
    }

    #[test]
    fn test_cardboard_toad1() {
        // `"hi"` is an expression, not a statement, so the grammar rejects it.
        let src = r#"
if ((abs(x) < (a.w+a2.w)) and
    (abs(y) < (a.h+a2.h)))
    then "hi" end
"#;
        let err = patch_lua(src).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("hi"), "{message}");
        assert!(message.contains("expected"), "{message}");
    }

    #[test]
    fn test_cardboard_toad2() {
        assert_patch(
            r#"
 if (self.sprites ~= nil) then
  self.sprite = self.sprites[self.sprites_index]
 end
"#,
            r#"
 if (self.sprites ~= nil) then
  self.sprite = self.sprites[self.sprites_index]
 end
"#,
        );
    }

    #[test]
    fn test_cardboard_toad3() {
        // This is a bug.
        // assert_patch(
        //     "accum += f.delay or self.delay",
        //     "accum = accum + f.delay or self.delay",
        // );

        // It should actually do this, but the corner cases are too many.
        assert_patch(
            "accum += f.delay or self.delay",
            "accum = accum + (f.delay or self.delay)",
        );

        assert_patch(
            "if true then accum += f.delay or self.delay end",
            "if true then accum = accum + (f.delay or self.delay) end",
        );
    }

    #[test]
    fn test_celeste0() {
        assert_patch(
            "if freeze>0 then freeze-=1 return end",
            "if freeze>0 then freeze = freeze - (1) return end",
        );
    }

    #[test]
    fn test_pooh_big_adventure0() {
        assert_patch(
            "if btnp(3) then self.choice += 1; result = true end",
            "if btnp(3) then self.choice = self.choice + (1); result = true end",
        );

        assert_patch("       i += 1", "       i = i + (1)");
    }

    #[test]
    fn test_plist0() {
        let lua = r#"
i += 1
local key = keys[i]
"#;
        let patched = ok(lua);
        assert!(patched.contains("i = i + (1)"));
    }

    #[test]
    fn test_find_includes() {
        let lua = r#"
#include a.p8
#include b.lua
"#;
        assert_eq!(
            find_includes(lua).collect::<Vec<_>>(),
            vec!["a.p8", "b.lua"]
        );
    }

    #[test]
    fn include_directive_skips_strings_and_comments() {
        let lua = "x += 1\nx = \"#include no.p8\"\n-- #include no.p8\n#include yes.p8\n";
        assert_eq!(
            find_includes(lua).collect::<Vec<_>>(),
            vec!["yes.p8".to_string()]
        );
        assert_eq!(
            patch_includes(lua, |path| format!("-- {path}").into()),
            "x += 1\nx = \"#include no.p8\"\n-- #include no.p8\n-- yes.p8\n"
        );
        assert_eq!(
            ok("#include foo.p8\nx += 1\n"),
            "#include foo.p8\nx = x + (1)\n"
        );
    }

    #[test]
    fn try_patch_includes_returns_the_first_error() {
        let err = try_patch_includes("#include missing.p8\n#include also.p8\n", |path| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                path.to_string(),
            ))
        })
        .unwrap_err();
        assert!(err.to_string().contains("missing.p8"), "{err}");
    }

    #[test]
    fn test_not_so_well0() {
        let src = "pos += (delta - thresh):map(function(v) return mid(0, v, 4) end)";
        let expected = "pos = pos + ((delta - thresh):map(function(v) return mid(0, v, 4) end))";
        assert!(parse::try_patch(src).is_ok());
        assert_eq!(ok(src), expected);
    }

    #[test]
    fn test_parser_shorthand_if_with_call() {
        let src = "if (o:ready()) o:go(1)\n";
        let expected = "if o:ready() then o:go(1) end\n";
        assert_eq!(parse::try_patch(src).unwrap(), expected);
    }

    #[test]
    fn test_parser_compound_or_and_call() {
        assert_eq!(
            parse::try_patch("accum += f.delay or self.delay").unwrap(),
            "accum = accum + (f.delay or self.delay)"
        );
        assert_eq!(
            parse::try_patch("pos += obj:step()").unwrap(),
            "pos = pos + (obj:step())"
        );
    }

    #[test]
    fn test_parser_slash_slash_inside_string() {
        let src = "x = \"http://example.com\" // hi\n";
        let expected = "x = \"http://example.com\" -- hi\n";
        assert_eq!(parse::try_patch(src).unwrap(), expected);
    }

    #[test]
    fn test_parser_shorthand_while() {
        assert_eq!(
            parse::try_patch("while (x > 0) x -= 1\n").unwrap(),
            "while x > 0 do x = x - (1) end\n"
        );
    }

    #[test]
    fn test_parser_print_explist() {
        assert_eq!(parse::try_patch("?a, b").unwrap(), "print(a, b)");
    }

    #[test]
    fn test_parser_binary_fraction_longer_than_a_nibble() {
        assert_eq!(parse::try_patch("a = 0b0.00001").unwrap(), "a = 0x0.08");
    }

    /// Pico-8 operators that are not already Lua.
    #[test_case(r"a = b \ c", "a = b // c" ; "floor division")]
    #[test_case("a = b ^^ c", "a = b ~ c" ; "xor")]
    #[test_case("a = b >>> c", "a = lshr(b, c)" ; "logical shift right")]
    #[test_case("a = b <<> c", "a = rotl(b, c)" ; "rotate left")]
    #[test_case("a = b >>< c", "a = rotr(b, c)" ; "rotate right")]
    #[test_case("a = @b", "a = peek(b)" ; "peek")]
    #[test_case("a = %b", "a = peek2(b)" ; "peek2")]
    #[test_case("a = $b", "a = peek4(b)" ; "peek4")]
    #[test_case("a = b ~ c", "a = b ~ c" ; "binary xor")]
    #[test_case(r"a \= b", "a = a // (b)" ; "floor division assign")]
    #[test_case("a ^^= b", "a = a ~ (b)" ; "xor assign")]
    #[test_case("a >>>= b", "a = lshr(a, (b))" ; "logical shift assign")]
    #[test_case("x = a ~= b", "x = a ~= b" ; "not equal stays")]
    #[test_case("a = ~b", "a = ~b" ; "bitwise not stays")]
    #[test_case("a = b % c", "a = b % c" ; "modulo stays")]
    #[test_case("a = b << c", "a = b << c" ; "shift stays")]
    fn dialect_operators(src: &str, expected: &str) {
        assert_eq!(ok(src), expected);
    }

    /// Ensure our capcity calculation is correct.
    fn splice(src: &str, edits: &[(usize, usize, String)]) -> String {
        let out = super::splice(src, edits);
        assert_eq!(out.capacity(), out.len());
        out
    }

    #[test]
    fn splice_with_no_edits_returns_the_source() {
        assert_eq!(splice("abc", &[]), "abc");
    }

    #[test]
    fn splice_keeps_the_tail_after_the_last_edit() {
        let edits = vec![(1, 2, "XY".to_string())];
        assert_eq!(splice("abcdef", &edits), "aXYcdef");
    }

    #[test]
    fn splice_keeps_the_gap_and_the_tail() {
        let edits = vec![(0, 3, "1".to_string()), (4, 7, "2".to_string())];
        assert_eq!(splice("one two three", &edits), "1 2 three");
    }

    #[test]
    fn patch_includes_keeps_the_code_after_the_directive() {
        assert_eq!(
            patch_includes("#include a.p8\nx = 1\n", |path| format!("-- {path}").into()),
            "-- a.p8\nx = 1\n"
        );
    }

    #[test]
    fn test_parse_failure_is_an_error() {
        let src = "@@";
        let err = patch_lua(src).unwrap_err();
        assert_eq!((err.line, err.column), (1, 1));
        let message = err.to_string();
        assert!(message.contains("@@"), "{message}");
        assert!(message.contains("expected"), "{message}");
    }
}

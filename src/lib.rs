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
use regex::{Regex, Replacer};
use std::{borrow::Cow, error::Error};
use find_matching_bracket::find_matching_paren;
use lazy_regex::regex;

// https://stackoverflow.com/a/79268946/6454690
fn replace_all_in_place<R: Replacer>(regex: &Regex, s: &mut Cow<'_, str>, replacer: R) {
    let new = regex.replace_all(s, replacer);
    if let Cow::Owned(o) = new {
        *s = Cow::Owned(o);
    } // Otherwise, no change was made.
}

/// Resolve the Pico-8 "#include path.p8" statements with possible errors.
///
/// If there are substitution errors, the first error will be returned.
pub fn try_patch_includes<'h, E: Error>(
    lua: impl Into<Cow<'h, str>>,
    mut resolve: impl FnMut(&str) -> Result<String, E>,
) -> Result<Cow<'h, str>, E> {
    let mut lua = lua.into();
    let mut error = None;

    replace_all_in_place(
        regex!(r"(?m)^\s*#include\s+(\S+)"),
        &mut lua,
        |caps: &regex::Captures| {
            match resolve(&caps[1]) {
                Ok(s) => s,
                Err(e) => {
                    // This is kind of pointless since the user will never get
                    // access to the string. I'm leaving here incase the results
                    // change to make it relevant later.
                    let result = format!("error(\"failed to include {:?}: {}\")", &caps[1], &e);
                    if error.is_none() {
                        error = Some(Err(e))
                    }
                    result
                }
            }
        },
    );
    error.unwrap_or(Ok(lua))
}

/// Returns true if the patch_output was patched by testing whether it is
/// `Cow::Owned`; a `Cow::Borrowed` implies it was not patched.
pub fn was_patched<'h>(patch_output: &Cow<'h, str>) -> bool {
    match patch_output {
        Cow::Owned(_) => true,
        Cow::Borrowed(_) => false,
    }
}

/// Resolve the Pico-8 "#include path.p8" statements without possible error.
pub fn patch_includes<'h>(
    lua: impl Into<Cow<'h, str>>,
    mut resolve: impl FnMut(&str) -> String,
) -> Cow<'h, str> {
    let mut lua = lua.into();
    replace_all_in_place(
        regex!(r"(?m)^\s*#include\s+(\S+)"),
        &mut lua,
        |caps: &regex::Captures| resolve(&caps[1]),
    );
    lua
}

/// Return each path from the the Pico-8 "#include path.p8" statements.
pub fn find_includes<'h>(
    lua: &'h str,
) -> impl Iterator<Item = String> {
    regex!(r"(?m)^\s*#include\s+(\S+)").captures_iter(&lua)
        .map(|caps: regex::Captures| caps[1].to_string())
}

/// Given a string with the Pico-8 dialect of Lua, it will convert that code to
/// plain Lua.
///
/// NOTE: This is not a full language parser, but a series of regular
/// expressions, so it is not guaranteed to work with every valid Pico-8
/// expression. But if it does not work, please file an issue with the failing
/// expression.
pub fn patch_lua<'h>(lua: impl Into<Cow<'h, str>>) -> Cow<'h, str> {
    let mut lua = lua.into();
    // Replace != with ~=.
    replace_all_in_place(regex!(r"!="), &mut lua, "~=");

    // Replace // with --.
    replace_all_in_place(regex!(r"//"), &mut lua, "--");

    // Replace unicode symbols for buttons.
    replace_all_in_place(
        regex!(r"(btnp?)\(\s*(\S+)\s*\)"),
        &mut lua,
        |caps: &regex::Captures| {
            let func = &caps[1];
            let symbol = caps[2].trim_end_matches("\u{fe0f}");
            let sub = match symbol {
                "⬅" => "0",
                "➡" => "1",
                "⬆" => "2",
                "⬇" => "3",
                "🅾" => "4",
                "❎" => "5",
                x => x,
            };
            format!("{func}({sub})")
        },
    );

    // Rewrite shorthand if statements.
    //
    // This is why using regex is not a great tool for parsing but because we
    // only need to match one line, we find the matching parenthesis and move on.
    replace_all_in_place(
        regex!(r"(?m)^(\s*)if\s*(\([^\n]*)$"),
        &mut lua,
        |caps: &regex::Captures| {
            let prefix = &caps[1];
            let line = &caps[2];

            if regex!(r"\bthen\b").is_match(line) {
                return caps[0].to_string();
            }
            if let Some(index) = find_matching_paren(line, 0) {
                let cond = &line[1..index];
                let body = &line[index + 1..].trim_start();
                let comment_start = body.find("--");
                if let Some(cs) = comment_start {
                    let (code, comment) = body.split_at(cs);
                    format!(
                        "{}if {} then {} end {}",
                        prefix,
                        cond,
                        code.trim_end(),
                        comment
                    )
                } else {
                    format!("{}if {} then {} end", prefix, cond, body)
                }
            } else {
                caps[0].to_string()
            }
        },
    );

    // Rewrite assignment operators (+=, -=, etc.).
    replace_all_in_place(regex!(r"(?m)([^-\s]\S*)\s*([+\-*/%])=\s*([^\n\r]+?)(\s*(\bend|\belse|;|--|$))"), &mut lua, "$1 = $1 $2 ($3)$4");

    // Replace "?expr" with "print(expr)".
    replace_all_in_place(regex!(r"(?m)^(\s*)\?([^\n\r]+)"), &mut lua, "${1}print($2)");

    // Convert binary literals to hex literals.
    replace_all_in_place(
        regex!(r"([^[:alnum:]_])0[bB]([01.]+)"),
        &mut lua,
        |caps: &regex::Captures| {
            let prefix = &caps[1];
            let bin = &caps[2];
            let mut parts = bin.split('.');

            let p1 = parts.next().unwrap_or("");
            let p2 = parts.next().unwrap_or("");

            let int_val = u64::from_str_radix(p1, 2).ok();
            let frac_val = if !p2.is_empty() {
                let padded = format!("{:0<4}", p2);
                u64::from_str_radix(&padded, 2).ok()
            } else {
                None
            };

            match (int_val, frac_val) {
                (Some(i), Some(f)) => format!("{}0x{:x}.{:x}", prefix, i, f),
                (Some(i), None) => format!("{}0x{:x}", prefix, i),
                _ => caps[0].to_string(),
            }
        },
    );
    lua
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_equal_replacement() {
        let lua = "if a != b then print(a) end";
        let patched = patch_lua(lua);
        assert!(patched.contains("a ~= b"));
    }

    #[test]
    fn test_comment_replacement() {
        let lua = "// this is a comment\nprint('hello')";
        let patched = patch_lua(lua);
        assert!(patched.contains("-- this is a comment"));
    }

    #[test]
    fn test_shorthand_if_rewrite() {
        let lua = "if (not b) i = 1\n";
        let expected = "if not b then i = 1 end\n";
        let patched = patch_lua(lua);
        assert_eq!(patched, expected);
    }

    #[test]
    fn test_shorthand_if_rewrite_comment() {
        let lua = "if (not b) i = 1 // hi\n";
        let expected = "if not b then i = 1 end -- hi\n";
        let patched = patch_lua(lua);
        assert_eq!(patched, expected);
    }

    #[test]
    fn test_shorthand_if_rewrite_and() {
        let lua = "if (not b and not c) i = 1\n";
        let expected = "if not b and not c then i = 1 end\n";
        let patched = patch_lua(lua);
        assert_eq!(patched, expected);
    }

    #[test]
    fn test_assignment_operator_rewrite() {
        let lua = "x += 1";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "x = x + (1)");
    }

    #[test]
    fn test_question_print_conversion0() {
        let lua = "?x";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "print(x)");
    }

    #[test]
    fn test_question_print_conversion() {
        let lua = "?x + y";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "print(x + y)");
    }

    #[test]
    fn test_binary_literal_conversion_integer() {
        let lua = "a = 0b1010";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "a = 0xa");
    }

    #[test]
    fn test_binary_literal_conversion_fractional() {
        let lua = "a = 0b1010.1";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "a = 0xa.8");
    }

    #[test]
    fn test_mixed_transforms() {
        let lua = r#"
        // comment
        if (a != b) x += 1
        ?x
        "#;
        let patched = patch_lua(lua);
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
        let patched = patch_lua(lua);
        // assert!(patched.is_borrowed());
        assert!(match patched {
            Cow::Owned(_) => false,
            Cow::Borrowed(_) => true,
        });
    }

    #[test]
    fn test_change_requires_allocation() {
        let lua = "x += 1";
        let patched = patch_lua(lua);
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
        let patched = patch_includes(lua, |path| format!("-- INCLUDE {}", path));
        assert!(patched.contains("-- INCLUDE blah.p8"), "{}", &patched);
    }

    #[test]
    fn test_bad_comment() {
        let lua = "--==configurations==--";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "--==configurations==--");
    }

    #[test]
    fn test_bad_if() {
        let lua =
            "if (ord(tb.str[tb.i],tb.char)!=32) sfx(tb.voice) -- play the voice sound effect.";
        let patched = patch_lua(lua);
        assert_eq!(
            patched.trim(),
            "if ord(tb.str[tb.i],tb.char)~=32 then sfx(tb.voice) end -- play the voice sound effect."
        );
    }

    #[test]
    fn test_bad_incr() {
        let lua = "tb.i+=1 -- increase the index, to display the next message on tb.str";
        let patched = patch_lua(lua);
        assert_eq!(
            patched.trim(),
            "tb.i = tb.i + (1) -- increase the index, to display the next message on tb.str"
        );
    }

    #[test]
    fn test_button() {
        let lua = "if btnp(➡️) or btn(❎) then";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "if btnp(1) or btn(5) then");
    }

    #[test]
    fn test_button2() {
        let lua = "if btnp(❎) then";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "if btnp(5) then");
    }

    #[test]
    fn test_button3() {
        let lua = "if btnp(🅾) then";
        let patched = patch_lua(lua);
        assert_eq!(patched.trim(), "if btnp(4) then");
    }

    fn assert_patch(unpatched: &str, expected_patched: &str) {
        let patched = patch_lua(unpatched);
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
        assert_patch(
            r#"
if ((abs(x) < (a.w+a2.w)) and
    (abs(y) < (a.h+a2.h)))
    then "hi" end
"#,
            r#"
if ((abs(x) < (a.w+a2.w)) and
    (abs(y) < (a.h+a2.h)))
    then "hi" end
"#,
        );
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
        assert_patch("accum += f.delay or self.delay",
                     "accum = accum + (f.delay or self.delay)");

        assert_patch("if true then accum += f.delay or self.delay end",
                     "if true then accum = accum + (f.delay or self.delay) end");
    }

    #[test]
    fn test_pooh_big_adventure0() {
        assert_patch("if btnp(3) then self.choice += 1; result = true end",
                     "if btnp(3) then self.choice = self.choice + (1); result = true end");

        assert_patch("       i += 1",
                     "       i = i + (1)");
    }

    #[test]
    fn test_plist0() {
        let lua = r#"
i += 1
local key = keys[i]
"#;
        let patched = patch_lua(lua);
        assert!(patched.contains("i = i + (1)"));
    }

    #[test]
    fn test_find_includes() {

        let lua = r#"
#include a.p8
#include b.lua
"#;
        assert_eq!(find_includes(&lua).collect::<Vec<_>>(), vec!["a.p8", "b.lua"]);
    }
}

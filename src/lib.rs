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
use lazy_regex::regex;
use std::{error::Error, borrow::Cow};

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
pub fn try_patch_includes<'h, E:Error>(lua: impl Into<Cow<'h, str>>, mut resolve: impl FnMut(&str) -> Result<String, E>) -> Result<Cow<'h, str>, E> {
    let mut lua = lua.into();
    let mut error = None;

    replace_all_in_place(regex!(r"#include\s+(\S+)"), &mut lua,
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
        });
    error.unwrap_or(Ok(lua))
}

/// Resolve the Pico-8 "#include path.p8" statements without possible error.
pub fn patch_includes<'h>(lua: impl Into<Cow<'h, str>>, mut resolve: impl FnMut(&str) -> String) -> Cow<'h, str>  {
    let mut lua = lua.into();
    replace_all_in_place(regex!(r"#include\s+(\S+)"), &mut lua,
        |caps: &regex::Captures| {
            resolve(&caps[1])
        });
    lua
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

    // Rewrite shorthand if statements.
    replace_all_in_place(
        regex!(r"(?m)^(\s*)if\s*\(([^)]*)\)\s*([^\n]*)\n"),
        &mut lua,
        |caps: &regex::Captures| {
            let prefix = &caps[1];
            let cond = &caps[2];
            let body = &caps[3];
            let comment_start = body.find("--");
            let has_keywords = regex!(r"\b(then|and|or)\b").is_match(body);

            if !has_keywords {
                if let Some(cs) = comment_start {
                    let (code, comment) = body.split_at(cs);
                    format!(
                        "{}if {} then {} end {}\n",
                        prefix,
                        cond,
                        code.trim_end(),
                        comment
                    )
                } else {
                    format!("{}if {} then {} end\n", prefix, cond, body)
                }
            } else {
                caps[0].to_string()
            }
        },
    );

    // Rewrite assignment operators (+=, -=, etc.).
    replace_all_in_place(regex!(r"(\S+)\s*([+\-*/%])="), &mut lua, "$1 = $1 $2");

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
        assert_eq!(patched.trim(), "x = x + 1");
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
            patched.contains("if a ~= b then x = x + 1 end"),
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
        assert!(patched.contains("-- INCLUDE blah.p8"));
    }
}

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
/// Licensed under the Zlib license:
///
/// This software is provided 'as-is', without any express or implied
/// warranty. In no event will the authors be held liable for any damages
/// arising from the use of this software.
///
/// Permission is granted to anyone to use this software for any purpose,
/// including commercial applications, and to alter it and redistribute it
/// freely, subject to the following restrictions:
///
/// 1. The origin of this software must not be misrepresented; you must not
///    claim that you wrote the original software. If you use this software
///    in a product, an acknowledgement in the product documentation would be
///    appreciated but is not required.
/// 2. Altered source versions must be plainly marked as such, and must not be
///    misrepresented as being the original software.
/// 3. This notice may not be removed or altered from any source distribution.
///
use regex::Regex;

/// Given a string with the Pico-8 dialect of Lua, it will attempt to convert
/// that code to plain Lua or return a regex error.
pub fn patch_lua(mut lua: String) -> Result<String, regex::Error> {
    // Replace != with ~=
    lua = lua.replace("!=", "~=");

    // Replace // with --
    lua = lua.replace("//", "--");

    // Rewrite shorthand if statements
    let re_if = Regex::new(r"(?m)^(\s*)if\s*\(([^)]*)\)\s*([^\n]*)\n")?;
    lua = re_if
        .replace_all(&lua, |caps: &regex::Captures| {
            let prefix = &caps[1];
            let cond = &caps[2];
            let body = &caps[3];
            let comment_start = body.find("--");
            let has_keywords = ["then", "and", "or"]
                .iter()
                .any(|&kw| Regex::new(&format!(r"\b{}\b", kw)).unwrap().is_match(body));

            if !has_keywords {
                if let Some(cs) = comment_start {
                    let (code, comment) = body.split_at(cs);
                    format!(
                        "{}if {} then {} end{}\n",
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
        })
        .to_string();

    // Rewrite assignment operators (+=, -=, etc.)
    let re_op = Regex::new(r"(\S+)\s*([+\-*/%])=")?;
    lua = re_op.replace_all(&lua, "$1 = $1 $2").to_string();

    // Replace "?expr" with "print(expr)"
    let re_print = Regex::new(r"(?m)^(\s*)\?([^\n\r]+)")?;
    lua = re_print.replace_all(&lua, "${1}print($2)").to_string();

    // Convert binary literals to hex literals
    let re_bin = Regex::new(r"([^[:alnum:]_])0[bB]([01.]+)")?;
    lua = re_bin
        .replace_all(&lua, |caps: &regex::Captures| {
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
        })
        .to_string();
    Ok(lua)
}

#[cfg(test)]
mod tests {
    use super::patch_lua;

    #[test]
    fn test_not_equal_replacement() -> Result<(), regex::Error> {
        let lua = "if a != b then print(a) end";
        let patched = patch_lua(lua.to_string())?;
        assert!(patched.contains("a ~= b"));
        Ok(())
    }

    #[test]
    fn test_comment_replacement() -> Result<(), regex::Error> {
        let lua = "// this is a comment\nprint('hello')";
        let patched = patch_lua(lua.to_string())?;
        assert!(patched.contains("-- this is a comment"));
        Ok(())
    }

    #[test]
    fn test_shorthand_if_rewrite() -> Result<(), regex::Error> {
        let lua = "\nif (not b) i = 1\n";
        let expected = "if not b then i = 1 end\n";
        let patched = patch_lua(lua.to_string())?;
        assert_eq!(patched, expected);
        Ok(())
    }

    #[test]
    fn test_assignment_operator_rewrite() -> Result<(), regex::Error> {
        let lua = "x += 1";
        let patched = patch_lua(lua.to_string())?;
        assert_eq!(patched.trim(), "x = x + 1");
        Ok(())
    }

    #[test]
    fn test_question_print_conversion0() -> Result<(), regex::Error> {
        let lua = "?x";
        let patched = patch_lua(lua.to_string())?;
        assert_eq!(patched.trim(), "print(x)");
        Ok(())
    }

    #[test]
    fn test_question_print_conversion() -> Result<(), regex::Error> {
        let lua = "?x + y";
        let patched = patch_lua(lua.to_string())?;
        assert_eq!(patched.trim(), "print(x + y)");
        Ok(())
    }

    #[test]
    fn test_binary_literal_conversion_integer() -> Result<(), regex::Error> {
        let lua = "a = 0b1010";
        let patched = patch_lua(lua.to_string())?;
        assert_eq!(patched.trim(), "a = 0xa");
        Ok(())
    }

    #[test]
    fn test_binary_literal_conversion_fractional() -> Result<(), regex::Error> {
        let lua = "a = 0b1010.1";
        let patched = patch_lua(lua.to_string())?;
        assert_eq!(patched.trim(), "a = 0xA.8");
        Ok(())
    }

    #[test]
    fn test_mixed_transforms() -> Result<(), regex::Error> {
        let lua = r#"
        // comment
        if (a != b) x += 1
        ?x
        "#;
        let patched = patch_lua(lua.to_string())?;
        assert!(patched.contains("-- comment"), "{}", patched);
        assert!(
            patched.contains("if a ~= b then x = x + 1 end"),
            "{}",
            patched
        );
        assert!(patched.contains("print(x)"), "{}", patched);
        Ok(())
    }
}

//! Try to translate each Pico-8 file and report the ones that do not parse.
use pico8_to_lua::patch_lua;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) fn execute<W: Write>(
    out: &mut W,
    recurse: bool,
    quiet: bool,
    files: &[PathBuf],
) -> io::Result<ExitCode> {
    let mut failed = 0usize;
    let mut checked = 0usize;
    let mut seen_dirs = HashSet::new();
    for path in files {
        visit(
            out,
            path,
            recurse,
            quiet,
            &mut seen_dirs,
            &mut failed,
            &mut checked,
        )?;
    }

    if failed == 0 {
        if !quiet {
            writeln!(out, "{} {} ok", checked, files_word(checked))?;
        }
        Ok(ExitCode::SUCCESS)
    } else {
        writeln!(out, "{failed} of {checked} failed")?;
        Ok(ExitCode::from(1))
    }
}

fn files_word(n: usize) -> &'static str {
    if n == 1 { "file" } else { "files" }
}

fn visit<W: Write>(
    out: &mut W,
    path: &Path,
    recurse: bool,
    quiet: bool,
    seen_dirs: &mut HashSet<PathBuf>,
    failed: &mut usize,
    checked: &mut usize,
) -> io::Result<()> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) => {
            *checked += 1;
            *failed += 1;
            return writeln!(out, "FAIL {}\n{err}", path.display());
        }
    };
    if meta.is_dir() {
        if !recurse {
            if !quiet {
                writeln!(
                    out,
                    "WARN {}: directory (pass -r to recurse)",
                    path.display()
                )?;
            }
            return Ok(());
        }
        if let Ok(canonical) = fs::canonicalize(path) {
            if !seen_dirs.insert(canonical) {
                return Ok(());
            }
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries.collect::<Result<Vec<_>, _>>(),
            Err(err) => Err(err),
        };
        let mut children = match entries {
            Ok(entries) => entries
                .into_iter()
                .map(|entry| entry.path())
                .collect::<Vec<_>>(),
            Err(err) => {
                *checked += 1;
                *failed += 1;
                return writeln!(out, "FAIL {}\n{err}", path.display());
            }
        };
        children.sort();
        for child in children {
            visit(out, &child, true, quiet, seen_dirs, failed, checked)?;
        }
        return Ok(());
    }
    if !is_source(path) {
        if !quiet {
            writeln!(out, "IGNORE {}", path.display())?;
        }
        return Ok(());
    }
    match translate(path) {
        Translated::Ok => {
            *checked += 1;
            if !quiet {
                writeln!(out, "ok {}", path.display())?;
            }
        }
        Translated::Warn(message) => {
            if !quiet {
                writeln!(out, "WARN {}: {message}", path.display())?;
            }
        }
        Translated::Fail(err) => {
            *checked += 1;
            *failed += 1;
            writeln!(out, "FAIL {}\n{err}", path.display())?;
        }
    }
    Ok(())
}

fn is_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("p8" | "lua" | "p8lua")
    )
}

enum Translated {
    Ok,
    Warn(String),
    Fail(String),
}

fn translate(path: &Path) -> Translated {
    let input = match fs::read_to_string(path) {
        Ok(input) => input,
        Err(err) => return Translated::Fail(err.to_string()),
    };
    let src = match lua_source(&input) {
        Ok(src) => src,
        Err(message) => return Translated::Warn(message),
    };
    match patch_lua(src) {
        Ok(_) => Translated::Ok,
        Err(err) => Translated::Fail(format!("{}:{}\n{err}", err.line, err.column)),
    }
}

/// Lua to translate. A `.p8` cart contributes its `__lua__` section, padded so
/// a parse error's line number is the line in the file.
fn lua_source(input: &str) -> Result<String, String> {
    if !input.starts_with("pico-8 cartridge") {
        return Ok(input.to_string());
    }
    let Some((header, rest)) = input.split_once("__lua__\n") else {
        return Err("no __lua__ section".to_string());
    };
    let lua = match rest.split_once("__gfx__") {
        Some((lua, _)) => lua,
        None => rest,
    };
    let pad = header.matches('\n').count() + 1;
    Ok(format!("{}{lua}", "\n".repeat(pad)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("pico8-check-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn run(recurse: bool, files: &[PathBuf]) -> (u8, String) {
        run_with(recurse, false, files)
    }

    fn run_with(recurse: bool, quiet: bool, files: &[PathBuf]) -> (u8, String) {
        let mut report = Vec::new();
        let code = execute(&mut report, recurse, quiet, files).unwrap();
        let code = match code {
            ExitCode::SUCCESS => 0,
            _ => 1,
        };
        (code, String::from_utf8(report).unwrap())
    }

    #[test]
    fn reports_which_carts_translate() {
        let dir = Dir::new("carts");
        let ok = dir.write(
            "ok.p8",
            "pico-8 cartridge // http://www.pico-8.com\nversion 43\n__lua__\nx += 1\n__gfx__\n@@\n",
        );
        let bad = dir.write(
            "bad.p8",
            "pico-8 cartridge // http://www.pico-8.com\nversion 43\n__lua__\n@@\n__gfx__\n",
        );

        let (code, report) = run(false, &[ok.clone(), bad.clone()]);
        assert_eq!(code, 1, "{report}");
        assert!(report.contains(&format!("ok {}", ok.display())), "{report}");
        assert!(
            report.contains(&format!("FAIL {}\n4:1\n", bad.display())),
            "{report}"
        );
        assert!(report.contains("@@"), "{report}");
        assert!(report.contains("1 of 2 failed"), "{report}");
    }

    #[test]
    fn all_ok_exits_zero() {
        let dir = Dir::new("lua");
        let lua = dir.write("plain.lua", "if (btn(❎)) x += 1\n");
        let (code, report) = run(false, &[lua]);
        assert_eq!(code, 0, "{report}");
        assert!(report.contains("1 file ok"), "{report}");
    }

    #[test]
    fn ignores_other_extensions() {
        let dir = Dir::new("ignore");
        let lua = dir.write("plain.lua", "x = 1\n");
        let notes = dir.write("notes.txt", "hello\n");
        let (code, report) = run(false, &[lua.clone(), notes.clone()]);
        assert_eq!(code, 0, "{report}");
        assert!(
            report.contains(&format!("ok {}", lua.display())),
            "{report}"
        );
        assert!(
            report.contains(&format!("IGNORE {}", notes.display())),
            "{report}"
        );
        assert!(report.contains("1 file ok"), "{report}");
    }

    #[test]
    fn warns_when_a_cart_has_no_lua_section() {
        let dir = Dir::new("nolua");
        let cart = dir.write(
            "gfx.p8",
            "pico-8 cartridge // http://www.pico-8.com\nversion 43\n__gfx__\n00\n",
        );
        let (code, report) = run(false, &[cart.clone()]);
        assert_eq!(code, 0, "{report}");
        assert!(
            report.contains(&format!("WARN {}: no __lua__ section", cart.display())),
            "{report}"
        );
        assert!(!report.contains("FAIL"), "{report}");
        assert!(report.contains("0 files ok"), "{report}");
    }

    #[test]
    fn warns_on_a_directory_without_recurse() {
        let dir = Dir::new("warn");
        dir.write("cart.p8", "x = 1\n");
        let (code, report) = run(false, &[dir.0.clone()]);
        assert_eq!(code, 0, "{report}");
        assert!(
            report.contains(&format!(
                "WARN {}: directory (pass -r to recurse)",
                dir.0.display()
            )),
            "{report}"
        );
        assert!(!report.contains("cart.p8"), "{report}");
        assert!(report.contains("0 files ok"), "{report}");
    }

    #[test]
    fn recurse_checks_source_files_only() {
        let dir = Dir::new("recurse");
        dir.write(
            "ok.p8",
            "pico-8 cartridge // http://www.pico-8.com\nversion 43\n__lua__\nx = 1\n__gfx__\n",
        );
        dir.write("notes.txt", "skip\n");
        fs::create_dir(dir.0.join("sub")).unwrap();
        let bad = dir.0.join("sub/bad.lua");
        fs::write(&bad, "@@\n").unwrap();
        let p8lua = dir.0.join("sub/also.p8lua");
        fs::write(&p8lua, "y = 1\n").unwrap();
        fs::write(dir.0.join("sub/readme.md"), "nope\n").unwrap();

        let (code, report) = run(true, &[dir.0.clone()]);
        assert_eq!(code, 1, "{report}");
        assert!(
            report.contains(&format!("ok {}", dir.0.join("ok.p8").display())),
            "{report}"
        );
        assert!(
            report.contains(&format!("IGNORE {}", dir.0.join("notes.txt").display())),
            "{report}"
        );
        assert!(
            report.contains(&format!("FAIL {}\n1:1\n", bad.display())),
            "{report}"
        );
        assert!(
            report.contains(&format!("ok {}", p8lua.display())),
            "{report}"
        );
        assert!(
            report.contains(&format!("IGNORE {}", dir.0.join("sub/readme.md").display())),
            "{report}"
        );
        assert!(!report.contains("WARN"), "{report}");
        assert!(report.contains("1 of 3 failed"), "{report}");
    }

    #[test]
    fn quiet_prints_only_failures() {
        let dir = Dir::new("quiet");
        let ok = dir.write("ok.lua", "x = 1\n");
        let bad = dir.write("bad.lua", "@@\n");
        let notes = dir.write("notes.txt", "hello\n");
        let cart = dir.write(
            "gfx.p8",
            "pico-8 cartridge // http://www.pico-8.com\nversion 43\n__gfx__\n00\n",
        );
        let (code, report) = run_with(false, true, &[ok, bad.clone(), notes, cart, dir.0.clone()]);
        assert_eq!(code, 1, "{report}");
        assert!(
            report.contains(&format!("FAIL {}\n1:1\n", bad.display())),
            "{report}"
        );
        assert!(report.contains("@@"), "{report}");
        assert!(!report.contains("ok "), "{report}");
        assert!(!report.contains("IGNORE"), "{report}");
        assert!(!report.contains("WARN"), "{report}");
        assert!(report.contains("1 of 2 failed"), "{report}");
    }

    #[test]
    fn quiet_with_no_failures_prints_nothing() {
        let dir = Dir::new("quiet-ok");
        let lua = dir.write("plain.lua", "x = 1\n");
        let notes = dir.write("notes.txt", "hello\n");
        let (code, report) = run_with(false, true, &[lua, notes]);
        assert_eq!(code, 0, "{report}");
        assert_eq!(report, "");
    }
}

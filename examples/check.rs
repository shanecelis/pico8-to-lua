//! Try to translate each Pico-8 file and report the ones that do not parse.
//!
//! ```sh
//! cargo run --example check a.p8 b.p8
//! cargo run --example check -r carts/
//! ```
use clap::Parser;
use pico8_to_lua::patch_lua;
use std::collections::HashSet;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(about = "Try to translate Pico-8 files and report the ones that do not parse")]
struct Args {
    /// Recurse into directories.
    #[arg(short, long)]
    recurse: bool,

    /// Files and directories to check.
    #[arg(required = true)]
    files: Vec<PathBuf>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let (code, report) = execute(args.recurse, &args.files);
    print!("{report}");
    code
}

fn execute(recurse: bool, files: &[PathBuf]) -> (ExitCode, String) {
    let mut failed = 0usize;
    let mut checked = 0usize;
    let mut seen_dirs = HashSet::new();
    let mut report = String::new();
    for path in files {
        visit(
            path,
            recurse,
            &mut seen_dirs,
            &mut failed,
            &mut checked,
            &mut report,
        );
    }

    if failed == 0 {
        let _ = writeln!(report, "{} {} ok", checked, files_word(checked));
        (ExitCode::SUCCESS, report)
    } else {
        let _ = writeln!(report, "{failed} of {checked} failed");
        (ExitCode::from(1), report)
    }
}

fn files_word(n: usize) -> &'static str {
    if n == 1 { "file" } else { "files" }
}

fn visit(
    path: &Path,
    recurse: bool,
    seen_dirs: &mut HashSet<PathBuf>,
    failed: &mut usize,
    checked: &mut usize,
    report: &mut String,
) {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) => {
            *checked += 1;
            *failed += 1;
            let _ = writeln!(report, "FAIL {}\n{err}", path.display());
            return;
        }
    };
    if meta.is_dir() {
        if !recurse {
            let _ = writeln!(
                report,
                "WARN {}: directory (pass -r to recurse)",
                path.display()
            );
            return;
        }
        if let Ok(canonical) = fs::canonicalize(path) {
            if !seen_dirs.insert(canonical) {
                return;
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
                let _ = writeln!(report, "FAIL {}\n{err}", path.display());
                return;
            }
        };
        children.sort();
        for child in children {
            visit(&child, true, seen_dirs, failed, checked, report);
        }
        return;
    }
    if !is_source(path) {
        let _ = writeln!(report, "IGNORE {}", path.display());
        return;
    }
    *checked += 1;
    match translate(path) {
        Ok(()) => {
            let _ = writeln!(report, "ok {}", path.display());
        }
        Err(err) => {
            *failed += 1;
            let _ = writeln!(report, "FAIL {}\n{err}", path.display());
        }
    }
}

fn is_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("p8" | "lua" | "p8lua")
    )
}

fn translate(path: &Path) -> Result<(), String> {
    let input = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let src = lua_source(&input)?;
    match patch_lua(src) {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{}:{}\n{err}", err.line, err.column)),
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
        let (code, report) = execute(recurse, files);
        let code = match code {
            ExitCode::SUCCESS => 0,
            _ => 1,
        };
        (code, report)
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
    fn no_args_prints_usage() {
        let err = Args::try_parse_from(["check"]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        let message = err.to_string();
        assert!(message.contains("Usage:"), "{message}");
    }

    #[test]
    fn recurse_flag() {
        let args = Args::try_parse_from(["check", "-r", "carts"]).unwrap();
        assert!(args.recurse);
        assert_eq!(args.files, vec![PathBuf::from("carts")]);
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
}

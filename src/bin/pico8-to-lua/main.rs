use pico8_to_lua::*;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod check;

const USAGE: &str = "\
Usage: pico8-to-lua [convert] [--lua-only] <file>
       pico8-to-lua check [-q] [-r] <path>...

Convert Pico-8 Lua to plain Lua, or check that files parse.

  convert        Rewrite a file. The word may be omitted.
  --lua-only     Print only the Lua section of a cart.
  check          Report files that do not parse.
  -q, --quiet    Print failures only.
  -r, --recurse  Recurse into directories.
  -h, --help     Print this help.";

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    args.next();
    match parse(args) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Convert { lua_only, filename }) => convert(lua_only, &filename),
        Ok(Command::Check {
            quiet,
            recurse,
            files,
        }) => {
            let (code, report) = check::execute(recurse, quiet, &files);
            print!("{report}");
            code
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.code)
        }
    }
}

#[derive(Debug)]
enum Command {
    Convert {
        lua_only: bool,
        filename: OsString,
    },
    Check {
        quiet: bool,
        recurse: bool,
        files: Vec<PathBuf>,
    },
    Help,
}

#[derive(Debug)]
struct CliError {
    message: String,
    code: u8,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<lexopt::Error> for CliError {
    fn from(err: lexopt::Error) -> Self {
        CliError {
            message: err.to_string(),
            code: 2,
        }
    }
}

fn missing_filename() -> CliError {
    CliError {
        message: "ERROR: Must provide filename argument".to_string(),
        code: 1,
    }
}

enum Leading {
    Short(char),
    Long(String),
    Value(OsString),
}

fn parse<I>(args: I) -> Result<Command, CliError>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    use lexopt::prelude::*;

    let mut parser = lexopt::Parser::from_args(args);
    let leading = match parser.next()? {
        None => return Err(missing_filename()),
        Some(Short('h') | Long("help")) => return Ok(Command::Help),
        Some(Value(cmd)) if cmd == "check" => return parse_check(parser),
        Some(Value(cmd)) if cmd == "convert" => None,
        Some(Value(path)) => Some(Leading::Value(path)),
        Some(Long(name)) => Some(Leading::Long(name.to_string())),
        Some(Short(ch)) => Some(Leading::Short(ch)),
    };
    parse_convert(parser, leading)
}

fn parse_check(mut parser: lexopt::Parser) -> Result<Command, CliError> {
    use lexopt::prelude::*;

    let mut quiet = false;
    let mut recurse = false;
    let mut files = Vec::new();
    while let Some(arg) = parser.next()? {
        match arg {
            Short('h') | Long("help") => return Ok(Command::Help),
            Short('q') | Long("quiet") => quiet = true,
            Short('r') | Long("recurse") => recurse = true,
            Value(path) => files.push(PathBuf::from(path)),
            other => return Err(other.unexpected().into()),
        }
    }
    if files.is_empty() {
        return Err(CliError {
            message: USAGE.to_string(),
            code: 2,
        });
    }
    Ok(Command::Check {
        quiet,
        recurse,
        files,
    })
}

fn take_arg(parser: &mut lexopt::Parser) -> Result<Option<Leading>, CliError> {
    use lexopt::prelude::*;

    Ok(match parser.next()? {
        None => None,
        Some(Value(path)) => Some(Leading::Value(path)),
        Some(Long(name)) => Some(Leading::Long(name.to_string())),
        Some(Short(ch)) => Some(Leading::Short(ch)),
    })
}

fn parse_convert(
    mut parser: lexopt::Parser,
    mut leading: Option<Leading>,
) -> Result<Command, CliError> {
    let mut lua_only = false;
    let mut filename = None;
    loop {
        let arg = if let Some(arg) = leading.take() {
            arg
        } else {
            match take_arg(&mut parser)? {
                Some(arg) => arg,
                None => break,
            }
        };
        match arg {
            Leading::Short('h') => return Ok(Command::Help),
            Leading::Long(name) if name == "help" => return Ok(Command::Help),
            Leading::Long(name) if name == "lua-only" => lua_only = true,
            Leading::Value(path) => {
                if filename.is_some() {
                    return Err(lexopt::Error::UnexpectedArgument(path).into());
                }
                filename = Some(path);
            }
            Leading::Short(ch) => {
                return Err(lexopt::Error::UnexpectedOption(format!("-{ch}")).into());
            }
            Leading::Long(name) => {
                return Err(lexopt::Error::UnexpectedOption(format!("--{name}")).into());
            }
        }
    }
    let Some(filename) = filename else {
        return Err(missing_filename());
    };
    Ok(Command::Convert { lua_only, filename })
}

fn split<'a>(s: &'a str, delimiter: &str) -> Vec<&'a str> {
    s.split(delimiter).collect()
}

fn convert(lua_only: bool, filename: &OsStr) -> ExitCode {
    let input = if filename == "-" {
        let mut buffer = String::new();
        if let Err(err) = io::stdin().read_to_string(&mut buffer) {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
        buffer
    } else {
        match fs::read_to_string(filename) {
            Ok(input) => input,
            Err(_) => {
                eprintln!("ERROR: File {} not found", Path::new(filename).display());
                return ExitCode::from(1);
            }
        }
    };

    let mut before_lua = None;
    let mut after_lua = None;

    let is_p8_file = input.starts_with("pico-8 cartridge");
    let pico8_lua = if is_p8_file {
        let before_delimiter = "__lua__\n";
        let after_delimiter = "__gfx__";

        let t1 = split(&input, before_delimiter);
        if t1.len() > 1 {
            before_lua = Some(t1[0].to_string());
            let t2 = split(t1[1], after_delimiter);
            if t2.len() > 1 {
                after_lua = Some(t2[1].to_string());
            }
            t2[0].to_string()
        } else {
            input
        }
    } else {
        input
    };

    let out_str = patch_lua(pico8_lua).unwrap_or_else(|err| {
        eprintln!("{err}");
        std::process::exit(1);
    });
    if is_p8_file && !lua_only {
        print!("{}__lua__\n{}", before_lua.unwrap_or("".into()), out_str);
        if after_lua.is_some() {
            print!("__gfx__{}", after_lua.unwrap());
        }
    } else {
        print!("{}", out_str);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_prints_usage() {
        let err = parse(["check"]).unwrap_err();
        assert_eq!(err.code, 2);
        assert!(err.to_string().contains("Usage:"), "{err}");
    }

    #[test]
    fn recurse_flag() {
        let Command::Check {
            quiet,
            recurse,
            files,
        } = parse(["check", "-r", "carts"]).unwrap()
        else {
            panic!("expected check");
        };
        assert!(recurse);
        assert!(!quiet);
        assert_eq!(files, vec![PathBuf::from("carts")]);
    }

    #[test]
    fn quiet_flag() {
        let Command::Check {
            quiet,
            recurse,
            files,
        } = parse(["check", "-q", "a.p8"]).unwrap()
        else {
            panic!("expected check");
        };
        assert!(quiet);
        assert!(!recurse);
        assert_eq!(files, vec![PathBuf::from("a.p8")]);
    }

    #[test]
    fn clustered_shorts() {
        let Command::Check {
            quiet,
            recurse,
            files,
        } = parse(["check", "-qr", "carts"]).unwrap()
        else {
            panic!("expected check");
        };
        assert!(quiet);
        assert!(recurse);
        assert_eq!(files, vec![PathBuf::from("carts")]);
    }

    #[test]
    fn dash_dash_ends_flags() {
        let Command::Check { recurse, files, .. } = parse(["check", "--", "-r"]).unwrap() else {
            panic!("expected check");
        };
        assert!(!recurse);
        assert_eq!(files, vec![PathBuf::from("-r")]);
    }

    #[test]
    fn a_filename_is_convert() {
        let Command::Convert { lua_only, filename } = parse(["cart.p8"]).unwrap() else {
            panic!("expected convert");
        };
        assert!(!lua_only);
        assert_eq!(filename, "cart.p8");
    }

    #[test]
    fn lua_only_before_the_file() {
        let Command::Convert { lua_only, filename } =
            parse(["convert", "--lua-only", "cart.p8"]).unwrap()
        else {
            panic!("expected convert");
        };
        assert!(lua_only);
        assert_eq!(filename, "cart.p8");
    }

    #[test]
    fn lua_only_after_the_file() {
        let Command::Convert { lua_only, filename } = parse(["cart.p8", "--lua-only"]).unwrap()
        else {
            panic!("expected convert");
        };
        assert!(lua_only);
        assert_eq!(filename, "cart.p8");
    }

    #[test]
    fn omitting_convert_accepts_lua_only_first() {
        let Command::Convert { lua_only, filename } = parse(["--lua-only", "cart.p8"]).unwrap()
        else {
            panic!("expected convert");
        };
        assert!(lua_only);
        assert_eq!(filename, "cart.p8");
    }

    #[test]
    fn a_file_named_check_needs_the_subcommand() {
        let Command::Convert { filename, .. } = parse(["convert", "check"]).unwrap() else {
            panic!("expected convert");
        };
        assert_eq!(filename, "check");
    }

    #[test]
    fn missing_filename() {
        for args in [Vec::<&str>::new(), vec!["convert"]] {
            let err = parse(args).unwrap_err();
            assert_eq!(err.code, 1);
            assert_eq!(err.to_string(), "ERROR: Must provide filename argument");
        }
    }

    #[test]
    fn help_lists_both_commands() {
        assert!(matches!(parse(["--help"]).unwrap(), Command::Help));
        assert!(matches!(parse(["check", "-h"]).unwrap(), Command::Help));
        assert!(USAGE.contains("pico8-to-lua [convert] [--lua-only] <file>"));
        assert!(USAGE.contains("pico8-to-lua check [-q] [-r] <path>..."));
    }
}

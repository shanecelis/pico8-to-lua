use pico8_to_lua::*;
use std::env;
use std::fs;
use std::io::{self, Read};

fn split<'a>(s: &'a str, delimiter: &str) -> Vec<&'a str> {
    s.split(delimiter).collect()
}

fn main() -> Result<(), io::Error> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("ERROR: Must provide filename argument");
        std::process::exit(1);
    }

    let filename = &args[1];
    let output_lua_only = args.len() > 2 && args[2] == "--lua-only";

    let input = if filename == "-" {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else {
        fs::read_to_string(filename).unwrap_or_else(|_| {
            eprintln!("ERROR: File {} not found", filename);
            std::process::exit(1);
        })
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

    let out_str = patch_lua(pico8_lua).unwrap();
    if is_p8_file && !output_lua_only {
        print!("{}__lua__\n{}", before_lua.unwrap_or("".into()), out_str);
        if after_lua.is_some() {
            print!("__gfx__{}", after_lua.unwrap());
        }
    }
    Ok(())
}

# pico8-to-lua

A library and command line tool to convert Pico-8's dialect of Lua to plain Lua. 

## Installation

### As a library

``` sh
cargo add pico8-to-lua
```

### As a command line tool

``` sh
cargo install pico8-to-lua
```

## Examples

### Patch a cart

``` sh
pico8-to-lua cart.p8 > patched-cart.p8
```

### Patch stdin

``` sh
echo "if (true) x+= 1" | pico8-to-lua -
if true then x = x + (1) end
```

### Patch the Code
``` rust
use pico8_to_lua::patch_lua;
assert_eq!(patch_lua("x += 1"), "x = x + (1)");
```

### Patch the Includes

``` rust
use pico8_to_lua::patch_includes;
fn comment_it(path: &str) -> String {
    format!("-- INCLUDE '{}'", path)
}
assert_eq!(patch_includes("#include file.p8", comment_it), "-- INCLUDE 'file.p8'");
```
It's recommended to patch the includes before patching the code in practice
because the includes may need patching as well.

## Omissions

This handles most of the Pico-8 dialect. However, it does not handle the
rotation operators: '>><' and '<<>'.

## Word of Caution

Don't go trusting this too much because it is merely a collection of regular
expressions and not a full blown language parser like it should be.

## Origin

This is a port of [Ben Wiley's
pico8-to-lua](https://github.com/benwiley4000/pico8-to-lua/) Lua tool to Rust.
Pico8-to-lua was originally derived from a function in Jez Kabanov's
[PICOLOVE](https://github.com/picolove/picolove/) project.

## License 

PICOLOVE is licensed under the Zlib license and so is Wiley's pico8-to-lua and
so this project is.



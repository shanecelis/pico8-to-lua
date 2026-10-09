# pico8-to-lua

A library and command line tool to convert Pico-8's Lua dialect into plain Lua. 

## Installation

### As a command line tool

``` sh
cargo install pico8-to-lua
```

### As a library

``` sh
cargo add pico8-to-lua --no-default-features
```

The default `cli` feature is not necessary for the library.

## Examples

### Patch a cart

This converts the `__lua__` section of the cart to plain Lua.

``` sh
pico8-to-lua convert cart.p8 > patched-cart.p8
```

Print only the Lua section:

``` console
$ cat cart.p8
pico-8 cartridge // http://www.pico-8.com
version 19
__lua__
if (true) x += 1
$ pico8-to-lua convert --lua-only cart.p8
if true then x = x + (1) end
```

### Patch stdin

``` console
$ echo "if (true) x+= 1" | pico8-to-lua convert -
if true then x = x + (1) end
```

### Check carts

One can check the syntax of many carts.

``` console
$ pico8-to-lua check a.p8 b.p8
ok a.p8
ok b.p8
2 files ok
```

`-q` prints failures only:

``` console
$ pico8-to-lua check -q bad.lua
FAIL bad.lua
1:1
 --> 1:1
  |
1 | @@
  | ^---
  |
  = expected chunk
1 of 1 failed
```

``` console
$ echo "if (true) x+= 1" | pico8-to-lua check -
ok -
1 file ok
```

Recurse with `-r`:

``` sh
pico8-to-lua check -r carts/
```

### Patch the Code
``` rust
use pico8_to_lua::patch_lua;
assert_eq!(patch_lua("x += 1").unwrap(), "x = x + (1)");
```

### Patch the Includes

``` rust
use pico8_to_lua::patch_includes;
use std::borrow::Cow;
/// This callback function accepts a path and returns a string. Typically it
/// might look at the file system and return the contents of file at the given
/// path. This function merely leaves a Lua comment for purposes of this test.
fn comment_it(path: &str) -> Cow<'static, str> {
    format!("-- INCLUDE '{}'", path).into()
}
assert_eq!(patch_includes("#include file.p8", comment_it), "-- INCLUDE 'file.p8'");
```
It's recommended to patch the includes before patching the code in practice
because the includes may need patching as well.

## Omissions

This handles most of the Pico-8 dialect. However, it does not handle the
rotation operators: '>><' and '<<>'.

## Word of Caution

`patch_lua` parses Pico-8 Lua and rewrites the dialect into standard Lua. A file
that does not parse returns an error.

## Origin

This is a port of [Ben Wiley's
pico8-to-lua](https://github.com/benwiley4000/pico8-to-lua/) Lua tool to Rust.
Pico8-to-lua was originally derived from a function in Jez Kabanov's
[PICOLOVE](https://github.com/picolove/picolove/) project.

## License 

PICOLOVE is licensed under the Zlib license and so is Wiley's pico8-to-lua and
so this project is.



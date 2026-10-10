# pico8-to-lua

A library and command line tool to convert Pico-8's Lua dialect into standard Lua.

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

## Transformations

`patch_lua` rewrites the dialect below. `&`, `|`, unary `~`, `<<`, and `>>` are already Lua, so they are copied through.

| Pico-8 | Lua | Done |
| --- | --- | --- |
| `a != b` | `a ~= b` | yes |
| `// comment` | `-- comment` | yes |
| `if (cond) stmt` | `if cond then stmt end` | yes |
| `if (cond) stmt else alt` | `if cond then stmt else alt end` | yes |
| `while (cond) stmt` | `while cond do stmt end` | yes |
| `if cond do`, `elseif cond do` | `then` in place of `do` | yes |
| `var += exp` | `var = var + (exp)` | yes |
| `var -= exp` | `var = var - (exp)` | yes |
| `var *= exp` | `var = var * (exp)` | yes |
| `var /= exp` | `var = var / (exp)` | yes |
| `?a, b` | `print(a, b)` | yes |
| `0b1010` | `0xa` | yes |
| `0b1010.1` | `0xa.8` | yes |
| `⬅` `➡` `⬆` `⬇` `🅾` `❎` | `0` `1` `2` `3` `4` `5` | yes |
| "⬅ ➡ ⬆ ⬇ 🅾 ❎" | "⬅ ➡ ⬆ ⬇ 🅾 ❎" | yes |
| fillp glyphs (`█` is `0.5`) | that pattern number | yes |
| `#include path` | result of callback `fn(&str) -> String` | yes |
| `a \ b` | `a // b` | yes |
| `a ^^ b` | `a ~ b` | yes |
| `a >>> b` | `lshr(a, b)` | yes |
| `a <<> b` | `rotl(a, b)` | yes |
| `a >>< b` | `rotr(a, b)` | yes |
| `@a` | `peek(a)` | yes |
| `%a` | `peek2(a)` | yes |
| `$a` | `peek4(a)` | yes |

Compound assignment uses those same operators. `a \= b` becomes `a = a // (b)`, and `a >>>= b` becomes `a = lshr(a, (b))`. Binary `a ~ b` is already Lua, so it is left as written.

`//`, `!=`, and glyphs inside a string stay as written. A glyph used as a name, such as `♥.x`, stays a name. `#include` is only recognized at the start of a line, and only `patch_includes` replaces it.

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



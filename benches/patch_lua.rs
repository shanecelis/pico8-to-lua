//! Compare the regex rewriter with the pest rewriter.
//!
//! ```sh
//! cargo bench
//! ```
//!
//! `regex` is the previous implementation. `parser` is [`pico8_to_lua::patch_lua`].

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use pico8_to_lua::{bench_parsed, bench_patch_lua_regex, patch_lua};

/// Plain Lua. Neither rewriter has anything to change.
const PLAIN: &str = r#"
function update_player(self)
  if self.grounded then
    self.dy = 0
  end
  self.x = self.x + self.dx
  self.y = self.y + self.dy
  local key = keys[i]
  return self.x
end
"#;

/// Pico-8 dialect: shorthand if/while, compound assignment, `!=`, `//`,
/// `?`, binary literals, and button glyphs.
const DIALECT: &str = concat!(
    r#"
function update_player(self)
  // trail comment
  if (btnp("#,
    "\u{274E}",
    r#")) self.jump += 1
  if freeze>0 then freeze-=1 return end
  if (not self.grounded) self.dy += 0.2
  accum += f.delay or self.delay
  pos += (delta - thresh):map(function(v) return mid(0, v, 4) end)
  if a != b then
    x += 1
  end
  while (x > 0) x -= 1
  ?x, y
  a = 0b1010
  b = 0b0.00001
  url = "http://example.com"
  if btnp("#,
    "\u{27A1}\u{FE0F}",
    r#") or btn("#,
    "\u{274E}",
    r#") then
    self.choice += 1
  end
  i += 1
  local key = keys[i]
end
"#
);

const CART_BYTES: usize = 32 * 1024;

fn cart(chunk: &str) -> String {
    let copies = CART_BYTES.div_ceil(chunk.len()).max(1);
    chunk.repeat(copies)
}

fn bench_patch(c: &mut Criterion) {
    let cases = [
        ("plain", PLAIN.to_string()),
        ("dialect", DIALECT.to_string()),
        ("plain_32kib", cart(PLAIN)),
        ("dialect_32kib", cart(DIALECT)),
    ];
    for (name, src) in &cases {
        assert!(
            bench_parsed(src),
            "{name} did not parse; the parser benchmark would time the regex fallback"
        );
    }

    let mut group = c.benchmark_group("patch_lua");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));

    for (name, src) in &cases {
        group.throughput(Throughput::Bytes(src.len() as u64));
        group.bench_with_input(BenchmarkId::new("regex", name), src, |b, src| {
            b.iter(|| black_box(bench_patch_lua_regex(black_box(src.as_str()))))
        });
        group.bench_with_input(BenchmarkId::new("parser", name), src, |b, src| {
            b.iter(|| black_box(patch_lua(black_box(src.as_str()))))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_patch);
criterion_main!(benches);

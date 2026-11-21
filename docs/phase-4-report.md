# Phase 4 report — streams and safety

Status: **complete for v0 semantics** (plan "Phase 4": all file/stdin/stdout
combinations, output safety, resource limits, corrupt-input tests). Verified
on macOS aarch64 in both debug (how CI's test lanes run) and release.
Native Windows x64 / macOS x64 *test execution* is delegated to CI: the
native jobs now run the conversion suites (`--test matrix --test streams
--release`) on windows-2022, macos-13 and macos-14 — that is the binary-safe
stdio verification for Windows, which a POSIX machine cannot provide.

## What shipped this phase

| piece | state |
| --- | --- |
| `--max-frames N` (plan §6.4) | frame-count budget checked from the container header **before** decode (exit 4); complements `--max-pixels-mp` / `--max-input-bytes` |
| `--timeout SECONDS` (plan §6.4 "where feasible") | deadline checked between pipeline stages (read → decode → encode → write), exit 4 `resource_limit`. Honest limitation, documented: blocking native codec calls and a stalled stdin cannot be interrupted in-process; the calling wrapper can enforce a hard kill |
| stdin size-limit fix | **real bug**: stdin reads used `take(max_bytes)`, which stops *at* the budget — the documented "input exceeds limit" branch was dead code and oversized stdin was silently truncated, then rejected downstream as *corrupt* (wrong diagnosis, wrong exit codes). Now reads one byte past the budget and refuses with exit 4 |
| tests/streams.rs (22 tests) | stream combinations (bare + subcommand forms), byte-exact stdout, ADR-006 discipline incl. `--json` and failure paths, overwrite/alias guards (incl. symlink aliasing), atomic write + no-litter checks, missing output dir, `--dry-run`, resource limits, corrupt/truncated input via file *and* stdin |
| tests/common/mod.rs | shared harness (binary override via `WEBP_CONVERT_BIN`, stdin-feeding runner, dir-litter checker); matrix.rs de-duplicated onto it |

## Interface rules the new suite pinned down

Already implemented, previously untested — now regression-protected:

- `-o -` (stdout output) with no `--format` **keeps the input format**;
  `--format FMT` overrides it. Both directions tested byte-exactly against
  file-output references.
- The `convert` subcommand takes only positional `INPUT OUTPUT`;
  `-i/-o` in that position is a usage error (exit 2).
- Failed runs write nothing to stdout — no bytes, no JSON — even in
  `-o -` mode (exit-3 error document lands on stderr).

## Bugs found and fixed during this phase

1. **stdin limit truncation** (above): silent truncation instead of exit 4,
   wrong error class (corrupt_input instead of resource_limit), limit
   effectively unenforceable via stdin.
2. No test previously exercised `--json` against a stdout *image* — the
   ADR-006 stderr-routing worked but was unverified for the failure path;
   one test now parses the stderr JSON document byte-for-byte.

## Verification evidence (this machine)

- `--test streams`: 22/22 release (~5 s) and debug (357 s — many debug
  -binary subprocess conversions; this is the CI-shaped run).
- `--test matrix`: 7/7 debug (158.7 s).
- Unit suites: core 18, cli args/io/engine 7; e2e integration 11.
- `cargo fmt --check` and `cargo clippy --workspace --all-targets -D
  warnings` clean.
- Manual binary-safety spot check: `target/release/webp-convert -i - -o
  out.webp < fixture.png` matches the file→file conversion byte-for-byte.

## Explicitly not done (honest list, carried forward)

- In-process hard kill of a wedged native codec call — impossible without
  process isolation; wrapper-level timeout (4D component, Phase 7+) is the
  enforcement point.
- Fuzzing, large-image/perf runs, clean-machine tests — Phase 6.
- Metadata write-through / animated write / TIFF pages — Phase 5.
- Signing/notarization — release infrastructure, Phase 6.

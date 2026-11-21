# AGENTS.md — webp (CLI repository)

Read this file, `docs/plan/webp-convert-master-plan.md`, and the ADRs in
`docs/adr/` before writing any code. The master plan is the authoritative
blueprint; ADRs record decisions and their status.

## Project identity

- This repository is the **CLI repository**: portable standalone
  image-conversion engine and CLI, provisional executable name
  `webp-convert`.
- The 4D component is a **separate repository** that pins released CLI
  assets. Do not put 4D code here.
- License: MIT (see `LICENSE`). Proprietary additions are out of scope.

## Non-negotiable rules (master plan §2)

1. CLI first; the CLI must be independently useful.
2. One conversion engine; never duplicate conversion logic in 4D.
3. No silent data loss: animation frames, TIFF pages, alpha, metadata,
   orientation, and significant color information must never be dropped
   without an explicit user decision and a diagnostic.
4. Portability is proven by running, never assumed from build success.
5. Implement against frozen contracts (ADRs), not against other agents'
   in-progress code.
6. The capability registry reports what the binary actually supports.
7. Treat every input image as untrusted; enforce resource limits.
8. Never claim a platform or test was verified unless it was actually run.
9. Small, reviewable changes; every change builds and tests.
10. Report honest completion reports (template in master plan §15).

## House rules

- Pin the Rust toolchain in `rust-toolchain.toml` once implementation
  starts; commit `Cargo.lock` for binaries.
- Do not add dependencies without recording their license in
  `docs/dependency-licenses.md`.
- Every bug found against a real image becomes a fixture under
  `tests/fixtures/regression/` where possible.
- The feasibility prototype lives in `tooling/codec-prototype/`. It is a
  scratch tool for evidence gathering. It must not become the product CLI
  and must never be released.
- Known codec landmine (found in prototype, 2026-11-21): libwebp
  **lossless** encoding rewrites the RGB of fully-transparent pixels when
  `exact=0` (the default). The engine must set `exact=1` when alpha must
  survive exactly.
- Exit codes and the JSON protocol (master plan §7) may change only while
  the schema version is `0`/draft; after the first tagged release they are
  frozen.

## Platforms

- Development host: macOS 27 (arm64). macOS x86_64 cross-compile and
  `lipo` Universal builds verified in the prototype.
- Windows x64 cannot be verified on this machine. Until a Windows runner
  proves a build portable, Windows is **unverified** — never report
  otherwise.

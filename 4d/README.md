# 4D component: `webp`

The 4D component wraps the independently versioned, pinned `webp-convert`
Rust CLI as a 4D component (`cs.WebP.Converter`). It follows the audited
`miyako/quickdraw` external-CLI adapter pattern; see ADR-012 for the
repository/release strategy (two release tracks in this one repo).

## Identity

| Key | Value |
| --- | --- |
| Repository | `miyako/webp` (this repo) |
| Component namespace | `WebP` |
| Feature class | `cs.WebP.Converter` |
| CLI basename | `webp-convert` |
| CLI lock pin | `4d/cli/asset-lock.json` |
| Component version range | `20.0.0` and up, **prefix-free** SemVer |
| Component release asset | exactly `webp.zip` |
| CLI version range | `1.0.0` and up, CLI-own convention (`v`-prefixed tags) |

Component versions are entirely independent from CLI versions. A component
release bundles (preinstalls) one verified CLI release asset.

## Layout

```text
4d/
  template.json            component identity + current version
  package.json             component package metadata (version mirrors template.json)
  release-policy.json      Apple notarization registry (schema 1)
  cli/
    asset-lock.json        pin: repository, CLI version, tag, commit, asset hashes
    assets/                gitignored: locally built CLI ZIP + manifest artifacts
  component/webp/
    Project/               4D component project (Sources/Classes, Documentation)
    Resources/             packaging inputs: en.lproj, licenses (bin/ is injected)
  scripts/                 local build/verify/package/test pipeline
  tests/                   host project + fixtures + Python runners
  workflows/               GitHub Actions port (draft until wired)
  dist/                    gitignored: webp.zip + acceptance evidence
  build/                   gitignored: transient pipeline state
```

## Wire contract (webp-convert 1.0.x)

The wrapped CLI speaks JSON protocol 1; the component decodes and validates it.

| Invocation | stdin/stdout behaviour |
| --- | --- |
| `webp-convert --version` | plain text `webp-convert <semver>\n` on stdout, exit 0, empty stderr (NOT JSON) |
| `capabilities --json` | one JSON document on stdout: `{"protocol":1,"ok":true,"operation":"capabilities","capabilities":[...]}` |
| `inspect <INPUT> --json` | `{"protocol":1,"ok":true,"operation":"inspect","input":{format,width,height,frames,has_alpha}}` |
| `-i <IN> -o <OUT> --json [opts]` | success as `convert` document with `output:{format,mode,bytes}`; failure as `{"protocol":1,"ok":false,"operation":"convert","error":{code,message}}` on **stdout** (empty stderr) |

- JSON (protocol) failures exit 2–7 with empty stderr; plain clap usage errors
  exit 2 with plain text on stderr and an empty stdout (the component maps
  those to `failureKind:"argument"`).
- Exit-code ↔ error-code cross-check table
  (src/crates/core/src/error.rs): 2 `invalid_arguments`/`missing_option`,
  3 `unsupported_input`/`corrupt_input`, 4 `resource_limit_exceeded`,
  5 `output_error`, 6 `internal_error`, 7 `protocol_error`.
- `inspect` takes only the positional INPUT (no other options);
  `convert` runs via the top-level bare form (`convert`/`inspect`
  subcommands accept only `--json`/`--quiet`).
- Safe value clamping: `--quality` is clamped by the CLI, so out-of-range
  numeric options produce a normal `convert` result, not a usage error.

## Component API

| Method | Value |
| --- | --- |
| `version()` | `{version:"1.0.0"}` (parsed from plain `--version` text, cross-checked against the lock) |
| `capabilities()` | full capability object |
| `inspect(input)` | `{protocol:1,"operation":"inspect","input":{...}}` |
| `convert(input; output; options)` | `{protocol:1,"operation":"convert","input":{...},"output":{...}}` |

Result envelope: `{ok:true, value}` or `{ok:false, failureKind, error:{code,message}}`
with `failureKind` one of `argument`, `process`, `asset`, `converter`, `protocol`.

Allowed `convert` options
(flags): `lossless`, `lossy`, `flatten`, `overwrite`, `dry-run`;
(numbers): `quality`, `method`, `alpha-quality`, `near-lossless`, `frame`,
`page`, `max-pixels-mp`, `max-input-bytes`, `max-frames`, `timeout`;
(texts): `format`, `to`, `from`, `background`, `metadata`, `orient`.
Any other key is rejected with `invalid_argument` before the process starts.

## Local pipeline

```bash
# 0. Prereq: build the CLI once for the pipeline (repo root):
#    cargo build --release --target universal2-apple-darwin
#    then create the gitignored local release bundle + manifests:
python3 4d/scripts/stage_cli.py        # build/verify/stage -> cli/asset-lock.json,
                                       # component Resources (bin + webp-assets.json)

# 1. tool4d host contract (source component):
python3 4d/scripts/test_4d.py \
  --tool4d "/path/to/tool4d.app/Contents/MacOS/tool4d"

# 2. Compile with the real free tool4d compiler:
python3 4d/scripts/compile_component.py \
  --tool4d "/path/to/tool4d.app/Contents/MacOS/tool4d"

# 3. Package the component (CLI preinstalled) -> 4d/dist/webp.zip
python3 4d/scripts/package_component.py --tag 20.0.0

# 4. One-shot acceptance (all of the above + compiled-host + stability,
#    emits build/acceptance.json and dist/webp.acceptance.json):
python3 4d/scripts/accept_local.py \
  --root . --tool4d "/path/to/tool4d.app/Contents/MacOS/tool4d"
```

All steps are order-safe and idempotent; `stage_cli.py` refuses to
write a lock whose hashes it could not freshly verify.

## Hosted acceptance (ADR-016)

Local acceptance is `partial` by construction (ADR-012): native Intel 4D,
Windows 4D and the published-download check run only outside it (`not_run`
above). The hosted pipeline — ported from the audited quickdraw precedent
and re-pointed at `4d/` paths and the webp lock — covers the rest:

- `.github/workflows/4d.yml` — reusable hosted acceptance job
  (macOS ARM64 runner): `ci_config.py download-tool4d` (SHA-256-pinned
  official download into `4d/.tools/tool4d.app`), pinned CLI asset
  download + staging, source host test, tool4d compile, packaging,
  compiled-host test on the final archive, then per-policy Apple signing
  + notarization (`sign_release.py`) when `release-policy.json` lists the
  component version (a signed build re-runs the compiled-host test on the
  signed ZIP). Evidence: `dist/webp.acceptance.json`, uploaded as an
  artifact.
- `.github/workflows/4d-build.yml` — manually dispatched (main only)
  Build: preflight (immutable commit, `ci_config.py validate --release`,
  tag availability, secret-free signing policy), hosted acceptance via
  `4d.yml`, then `release_assets.py --create-tag --publish` which
  fail-closed-verifies every asset against the accepted evidence and
  creates the prefix-free tag (e.g. `20.0.0`) before
  `gh release create` with `webp.zip` + acceptance evidence (+ `webp.dmg`
  and `webp-signing.json` on signed builds).
- `.github/workflows/ci.yml` runs the unsigned acceptance from `4d.yml`
  on every push/PR, gated on the repository variable `FOURD_ENABLED=true`
  (skipped on forks and when unset).

Repository configuration required once:

| Item | Kind | Purpose |
|---|---|---|
| `FOURD_ENABLED` | variable | `true` enables the 4D acceptance + release jobs |
| `TOOL4D_URL` | variable | official pinned tool4d app-archive URL (`resources-download.4d.com` / `builds2.4d.com`) |
| `TOOL4D_SHA256` | variable | SHA-256 of that exact archive |
| `APPLE_DEVELOPER_ID_CERTIFICATE` | secret | base64 PKCS#12 Developer ID Application certificate |
| `APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD` | secret | PKCS#12 passphrase |
| `KEYCHAIN_PASSWORD` | secret | password for the ephemeral CI keychain (any strong random value) |
| `NOTARYTOOL_APPLE_ID` | secret | Apple ID for notarytool |
| `NOTARYTOOL_TEAM_ID` | secret | App Store Connect team ID |
| `NOTARYTOOL_PASSWORD` | secret | app-specific password for notarytool |
| `CODESIGN_APPLE_ID` | secret (optional) | pins the identity (name or SHA-1) |

`4d/scripts/set-release-secrets.sh owner/repo` mirrors these from the
local login Keychain into GitHub secrets (user-run helper; agents must
not execute it). Signing is gated two ways: the version must appear in
`4d/release-policy.json` → `apple_notarized_versions`, or the caller must
pass `--require-apple-signing`, which can only upgrade, never downgrade
publication policy.

## Release steps (component 20.0.0)

The hosted Build workflow automates steps 3-4 below; 1-2 remain
manual because the asset lock is a reviewed, committed pin.

1. Push `main`, then push the CLI tag `v1.0.0` and publish the CLI release
   assets (`webp-convert-1.0.0-*.zip` + manifests) that `asset-lock.json`
   already pins. (User action; `gh` is not required — release by tag upload.)
2. Verify `4d/cli/asset-lock.json` hashes against the uploaded assets.
3. Run the **build** workflow (Actions → build → Run workflow, `main`).
   It accepts, (optionally signs + notarizes), creates tag `20.0.0`
   and publishes the release with `webp.zip`.
4. Optional/accepted-gap: macOS signing is available via the workflow
   secrets above; a version is signed only once listed in
   `release-policy.json` → `apple_notarized_versions`. Unsigned builds
   keep the Gatekeeper right-click → Open caveat.
5. Windows is out of scope for 20.0.0: no Windows CLI release exists yet;
   the lock has no `windows` entry and the component hard-fails asset
   verification on Windows rather than running unverified code.

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
# 1. Build/refresh the CLI asset pack and asset-lock (from target/universal)
python3 4d/scripts/build_cli_asset.py

# 2. Run the tool4d host smoke (source component)
python3 4d/scripts/test_4d.py \
  --tool4d "/path/to/tool4d.app/Contents/MacOS/tool4d"

# 3. Package the component (CLI preinstalled) -> 4d/dist/webp.zip
python3 4d/scripts/package_component.py

# 4. Validate the produced ZIP (unzip safety + resource checks)
python3 4d/scripts/package_component.py --verify-only
```

All steps are order-safe and idempotent; `build_cli_asset.py` refuses to
write a lock whose hashes it could not freshly verify.

## Release steps (component 20.0.0)

1. Push `main`, then push the CLI tag `v1.0.0` and publish the CLI release
   assets (`webp-convert-1.0.0-*.zip` + manifests) that `asset-lock.json`
   already pins. (User action; `gh` is not required — release by tag upload.)
2. Verify `4d/cli/asset-lock.json` hashes against the uploaded assets.
3. Tag `20.0.0` (no `v`) on the same commit whose file tree contains the
   lock; attach `4d/dist/webp.zip` as the release asset named exactly
   `webp.zip`.
4. Optional/accepted-gap: the CLI binary inside `webp.zip` is not Developer
   ID signed/notarized; `release-policy.json` records no notarized versions.
   Gatekeeper will require right-click → Open, or a future signed rebuild
   (ADR: signing decision pending).
5. Windows is out of scope for 20.0.0: no Windows CLI release exists yet;
   the lock has no `windows` entry and the component hard-fails asset
   verification on Windows rather than running unverified code.

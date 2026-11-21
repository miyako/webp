# Converter

`cs.WebP.Converter.new(target)` wraps the independently versioned, pinned
Rust `webp-convert` executable. This is not a plug-in.
`target` is `"macOS-universal"` (or omitted on macOS) or explicitly
`"Windows-x64"` on Windows. Windows is not provided by component 20.0.0
(no verified Windows CLI asset exists yet); resolution fails on Windows.

| Method | Value |
| --- | --- |
| `version()` | `{version:"1.0.0"}` parsed from the CLI's plain `--version` text, cross-checked against the pinned asset metadata |
| `capabilities()` | full capability object (`{"protocol":1,"operation":"capabilities","capabilities":[...]}`) |
| `inspect(input)` | `{"protocol":1,"operation":"inspect","input":{format,width,height,frames,has_alpha}}` |
| `convert(input; output; options)` | `{"protocol":1,"operation":"convert","input":{...},"output":{format,mode,bytes}}` |

`input`/`output` paths are nonempty Text or `4D.File`, never Folder or plain
Object. File objects are expanded to native paths. **Use File objects or
absolute paths**: relative Text paths resolve in the bundled executable's
parent directory, not the host data directory. Unicode, quotes and shell
punctuation are literal (arguments are CRT-quoted byte-exactly); NUL is
rejected everywhere.

## Result envelope

Every method returns one object:

- success: `{ok:true, value:<decoded protocol document>}`
- failure: `{ok:false, failureKind, error:{code, message}}`

`failureKind` is one of:

- `argument` — the call was rejected before starting the CLI
  (`invalid_argument`)
- `process` — the CLI could not be resolved or launched (`unsupported_architecture`,
  `missing_executable`, `launch_failed`, `process_error`, `process_terminated`,
  `cli_error`)
- `asset` — missing or mismatched verified asset metadata (`asset_mismatch`,
  reported on construction of any invocation)
- `converter` — the CLI ran and reported an error (`invalid_arguments`,
  `missing_option`, `unsupported_input`, `corrupt_input`,
  `resource_limit_exceeded`, `output_error`, `internal_error`,
  `protocol_error`)
- `protocol` — the CLI's output violated the pinned wire contract
  (`invalid_protocol`)

## convert options

Flags (Boolean, emitted only when true): `lossless`, `lossy`, `flatten`,
`overwrite`, `dry-run`.
Numbers (Number or decimal Text): `quality`, `method`, `alpha-quality`,
`near-lossless`, `frame`, `page`, `max-pixels-mp` (finite decimal),
`max-input-bytes`, `max-frames`, `timeout` (integers).
Texts: `format`, `to`, `from`, `background`, `metadata`, `orient`.

Any other option is rejected with `invalid_argument` before the process
starts. There is no `image`-style passthrough; the allowlist is exactly the
webp-convert 1.0.x surface. `inspect` takes no options.

The component fetches `version()` + `capabilities()` once lazily before the
first `convert`/`inspect` to fail fast on a broken or unverified asset; each
call still re-verifies the binary digest at invocation time.

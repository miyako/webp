property _cli : cs._CLI
property _capabilities : Object

Class constructor($target : Text)
	This._cli:=cs._CLI.new("webp-convert"; $target)

Function version()->$result : Object
	$result:=This._invoke(New collection("--version"); "version")

Function capabilities()->$result : Object
	$result:=This._invoke(New collection("capabilities"; "--json"); "capabilities")
	If ($result.ok)
		This._capabilities:=$result.value
	End if

Function inspect($input : Variant)->$result : Object
	var $path : Object
	$path:=This._path($input)
	If (Not($path.ok))
		return $path
	End if
	$result:=This._invoke(New collection("inspect"; $path.path; "--json"); "inspect")

Function convert($input : Variant; $output : Variant; $options : Variant)->$result : Object
	var $built : Object
	$built:=This._arguments($input; $output; $options)
	If ($built.ok)
		$result:=This._invoke($built.arguments; "convert")
	Else
		$result:=$built
	End if

Function _invoke($arguments : Collection; $command : Text)->$result : Object
	var $resolved; $checked : Object
	// make sure the verified asset is present before any invocation;
	// capabilities are not needed to validate the wire contract
	$resolved:=This._cli.resolve()
	If (Not($resolved.ok))
		$resolved.failureKind:="process"
		return $resolved
	End if
	$checked:=This._checkAsset($resolved.file; Folder("/RESOURCES").file("webp-assets.json"))
	If (Not($checked.ok))
		return $checked
	End if
	$result:=This._decode(cs._Process.new().execute($resolved.file; $arguments); $command)
	If ($result.ok & ($command="version"))
		If ($result.value.version#$checked.version)
			$result.ok:=False
			$result.failureKind:="asset"
			$result.error:=New object("code"; "asset_mismatch"; "message"; "CLI version differs from verified release metadata")
			$result.value:=Null
		End if
	End if

Function _arguments($input : Variant; $output : Variant; $options : Variant)->$result : Object
	var $args; $flags; $numbers; $texts : Collection
	var $key; $text : Text
	var $value : Variant
	var $in; $out : Object
	$flags:=New collection("lossless"; "lossy"; "flatten"; "overwrite"; "dry-run")
	$numbers:=New collection("quality"; "method"; "alpha-quality"; "near-lossless"; "frame"; "page"; "max-pixels-mp"; "max-input-bytes"; "max-frames"; "timeout")
	$texts:=New collection("format"; "to"; "from"; "background"; "metadata"; "orient")
	$in:=This._path($input)
	If (Not($in.ok))
		return $in
	End if
	$out:=This._path($output)
	If (Not($out.ok))
		return $out
	End if
	$args:=New collection("-i"; $in.path; "-o"; $out.path; "--json")
	If (($options=Null) | (Value type($options)=Is undefined))
		return New object("ok"; True; "arguments"; $args)
	End if
	If (Value type($options)#Is object)
		return This._argumentError("Options must be an Object")
	End if
	For each ($key; OB Keys($options))
		If (($flags.indexOf($key)<0) & ($numbers.indexOf($key)<0) & ($texts.indexOf($key)<0))
			return This._argumentError("Unsupported option for convert: "+$key)
		End if
		$value:=$options[$key]
		Case of
			: ($flags.indexOf($key)>=0)
				If (Value type($value)#Is boolean)
					return This._argumentError($key+" must be Boolean")
				End if
				If ($value)
					$args.push("--"+$key)
				End if
			: ($numbers.indexOf($key)>=0)
				Case of
					: (Value type($value)=Is text)
						$text:=$value
					: ((Value type($value)=Is real) | (Value type($value)=Is longint))
						$text:=JSON Stringify($value)
					Else
						return This._argumentError($key+" must be a Number or decimal Text")
				End case
				If ($key="max-pixels-mp")
					If (Not(Match regex("^[+]?[0-9]+([.][0-9]+)?$"; $text)))
						return This._argumentError("max-pixels-mp must be a finite decimal Text or Number")
					End if
				Else
					If (Not(Match regex("^-?[0-9]+$"; $text)))
						return This._argumentError($key+" must be an integer")
					End if
				End if
				$args.push("--"+$key; $text)
			: ($texts.indexOf($key)>=0)
				If (Value type($value)#Is text)
					return This._argumentError($key+" must be Text")
				End if
				If (Position(Char(0); $value; *)>0)
					return This._argumentError("NUL is not permitted in options")
				End if
				$args.push("--"+$key; $value)
		End case
	End for each
	$result:=New object("ok"; True; "arguments"; $args)

Function _checkAsset($binary : 4D.File; $metadata : 4D.File)->$result : Object
	var $lock; $version : Variant
	var $bytes : Blob
	var $previous; $digest; $platform : Text
	var $errors : Collection
	var CLI_LastErrors : Collection
	$result:=New object("ok"; False; "failureKind"; "asset"; "error"; New object("code"; "asset_mismatch"; "message"; "Missing or mismatched verified CLI asset metadata"))
	If (Not($metadata.exists))
		return
	End if
	$previous:=Method called on error(ek local)
	CLI_LastErrors:=Null
	ON ERR CALL("_CaptureError"; ek local)
	$lock:=JSON Parse($metadata.getText())
	$bytes:=$binary.getContent()
	$digest:=Lowercase(Generate digest($bytes; SHA256 digest))
	ON ERR CALL($previous; ek local)
	$errors:=CLI_LastErrors
	CLI_LastErrors:=Null
	If (($errors#Null) | (Value type($lock)#Is object))
		$result.error.details:=$errors
		return
	End if
	If ((Value type($lock.assets)#Is object) | (Value type($lock.version)#Is text) | (Value type($lock.source_revision)#Is text))
		return
	End if
	If (Not(Match regex("^[0-9a-f]{40}$"; $lock.source_revision)))
		return
	End if
	$version:=$lock.version
	$platform:="macos"
	If (Is Windows)
		$platform:="windows"
	End if
	$entry:=$lock.assets[$platform]
	If (Value type($entry)#Is object)
		return
	End if
	If (Value type($entry.binary_sha256)#Is text)
		return
	End if
	If ($entry.binary_sha256#$digest)
		return
	End if
	$result:=New object("ok"; True; "version"; $version)

Function _argumentError($message : Text)->$result : Object
	$result:=New object("ok"; False; "failureKind"; "argument"; "error"; New object("code"; "invalid_argument"; "message"; $message))

Function _path($value : Variant)->$result : Object
	var $path : Text
	Case of
		: (Value type($value)=Is text)
			$path:=$value
		: (Value type($value)=Is object)
			If (Not(OB Instance of($value; 4D.File)))
				return This._argumentError("Paths must be Text or 4D.File")
			End if
			$path:=File($value.platformPath; fk platform path).path
			If (Is Windows)
				$path:=$value.platformPath
			End if
		Else
			return This._argumentError("Paths must be Text or 4D.File")
	End case
	If (($path="") | (Position(Char(0); $path; *)>0))
		return This._argumentError("Paths must be nonempty and contain no NUL")
	End if
	$result:=New object("ok"; True; "path"; $path)

Function _protocolError($result : Object; $message : Text)->$decoded : Object
	$result.ok:=False
	$result.value:=Null
	$result.failureKind:="protocol"
	$result.error:=New object("code"; "invalid_protocol"; "message"; $message)
	$decoded:=$result

Function _jsonDocument($text : Text)->$payload : Variant
	// Parse one complete JSON document; the CLI pretty-prints, one document
	// per invocation, ending in a final LF. Returns Null when $text is not a
	// complete, balanced document.
	var $previous : Text
	var $parseErrors : Collection
	var $i : Integer
	var $inString; $escape : Boolean
	var $character : Text
	var CLI_LastErrors : Collection
	If (Value type($text)#Is text)
		return
	End if
	If ((Length($text)=0) | (Substring($text; Length($text); 1)#"\n"))
		return
	End if
	$inString:=False
	$escape:=False
	For ($i; 1; Length($text))
		$character:=Substring($text; $i; 1)
		If ($inString)
			If ($escape)
				$escape:=False
			Else
				If ($character="\\")
					$escape:=True
				Else
					If ($character="\"")
						$inString:=False
					End if
				End if
			End if
		Else
			If ($character="\"")
				$inString:=True
			End if
		End if
	End for
	If ($inString)
		return
	End if
	$previous:=Method called on error(ek local)
	CLI_LastErrors:=Null
	ON ERR CALL("_CaptureError"; ek local)
	$payload:=JSON Parse($text)
	ON ERR CALL($previous; ek local)
	$parseErrors:=CLI_LastErrors
	CLI_LastErrors:=Null
	If ($parseErrors#Null)
		$payload:=Null
	End if

Function _exitCodeAllows($exitCode : Integer; $errorCode : Text)->$allowed : Boolean
	var $codes : Collection
	Case of
		: ($exitCode=2)
			$codes:=New collection("invalid_arguments"; "missing_option")
		: ($exitCode=3)
			$codes:=New collection("unsupported_input"; "corrupt_input")
		: ($exitCode=4)
			$codes:=New collection("resource_limit_exceeded")
		: ($exitCode=5)
			$codes:=New collection("output_error")
		: ($exitCode=6)
			$codes:=New collection("internal_error")
		: ($exitCode=7)
			$codes:=New collection("protocol_error")
		Else
			$codes:=Null
	End case
	If ($codes=Null)
		$allowed:=False
	Else
		$allowed:=($codes.indexOf($errorCode)>=0)
	End if

Function _decode($result : Object; $command : Text)->$decoded : Object
	var $payload : Variant
	var $entry; $field : Variant
	var $stream : Text
	// _Process wraps nonzero/failed executions; for --json operations the CLI
	// emits the failure document on stdout, so cli_error still carries usable
	// streams. Any non-cli error is a process failure.
	If (Not($result.ok))
		If ($result.error.code#"cli_error")
			$result.failureKind:="process"
			$result.value:=Null
			return $result
		End if
	End if
	If ((Value type($result.stdout)#Is text) | (Value type($result.stderr)#Is text))
		return This._protocolError($result; "CLI streams must be Text")
	End if
	$stream:=$result.stdout
	If ($command="version")
		// webp-convert --version is plain text, not JSON protocol 1
		If (($result.exitCode#0) | ($result.stderr#""))
			return This._protocolError($result; "Version check failed")
		End if
		If (Not(Match regex("^webp-convert [0-9]+[.][0-9]+[.][0-9]+[^\r\n]*\n$"; $stream)))
			return This._protocolError($result; "Version response must be one `webp-convert <semver>` line")
		End if
		$result.value:=New object("version"; Substring($stream; 14; Length($stream)-14))
		$decoded:=$result
		return $decoded
	End if
	$payload:=This._jsonDocument($stream)
	If (Value type($payload)#Is object)
		// The only sanctioned non-JSON case is a plain usage error:
		// empty stdout, plain clap text on stderr, exit 2. Parse failures
		// and non-object documents (true, 1, [ ], strings) cannot be
		// validated below without raising, so they are protocol errors;
		// empty stdout can never parse to a non-object document.
		If (($result.exitCode=2) & ($stream="") & ($result.stderr#""))
			$result.ok:=False
			$result.value:=Null
			$result.failureKind:="argument"
			$result.error:=New object("code"; "invalid_arguments"; "message"; $result.stderr)
			$decoded:=$result
			return $decoded
		End if
		return This._protocolError($result; "CLI response must be one JSON document on stdout")
	End if
	If ((Value type($payload.protocol)#Is real) & (Value type($payload.protocol)#Is longint))
		return This._protocolError($result; "Unsupported CLI protocol version")
	End if
	If ($payload.protocol#1)
		return This._protocolError($result; "Unsupported CLI protocol version")
	End if
	If (Value type($payload.ok)#Is boolean)
		return This._protocolError($result; "Missing ok flag")
	End if
	If (Value type($payload.operation)#Is text)
		return This._protocolError($result; "Missing operation")
	End if
	If (Not($payload.ok))
		// failure document on stdout
		If ($result.stderr#"")
			return This._protocolError($result; "Protocol failures must have empty stderr")
		End if
		If (Value type($payload.error)#Is object)
			return This._protocolError($result; "Missing converter error")
		End if
		If ((Value type($payload.error.code)#Is text) | (Value type($payload.error.message)#Is text))
			return This._protocolError($result; "Invalid converter error")
		End if
		If ($payload.operation#$command)
			return This._protocolError($result; "Converter error operation mismatch")
		End if
		If ((Value type($result.exitCode)#Is real) & (Value type($result.exitCode)#Is longint))
			return This._protocolError($result; "Converter error exit code invalid")
		End if
		If (Not(This._exitCodeAllows($result.exitCode; $payload.error.code)))
			return This._protocolError($result; "Converter status and error code disagree")
		End if
		$result.ok:=False
		$result.failureKind:="converter"
		$result.error:=$payload.error
		$result.value:=$payload
		$decoded:=$result
		return $decoded
	End if
	// success document
	If (($result.stderr#"") | ($result.exitCode#0))
		return This._protocolError($result; "Success requires exit zero and empty stderr")
	End if
	Case of
		: ($command="capabilities")
			If ($payload.operation#"capabilities")
				return This._protocolError($result; "Unexpected operation")
			End if
			If (Value type($payload.capabilities)#Is collection)
				return This._protocolError($result; "Missing capabilities")
			End if
			If ($payload.capabilities.length=0)
				return This._protocolError($result; "Missing capabilities")
			End if
			For each ($entry; $payload.capabilities)
				If (Value type($entry)#Is object)
					return This._protocolError($result; "Invalid capabilities entry")
				End if
				If (Value type($entry.format)#Is text)
					return This._protocolError($result; "Invalid capability format")
				End if
			End for each
		: ($command="inspect")
			If ($payload.operation#"inspect")
				return This._protocolError($result; "Unexpected operation")
			End if
			If (Value type($payload.input)#Is object)
				return This._protocolError($result; "Missing inspection")
			End if
			For each ($field; New collection("format"; "width"; "height"; "frames"; "has_alpha"))
				If (Not(OB Is defined($payload.input; $field)))
					return This._protocolError($result; "Invalid inspection: "+$field)
				End if
			End for each
			If (Value type($payload.input.format)#Is text)
				return This._protocolError($result; "Invalid inspection format")
			End if
			For each ($field; New collection("width"; "height"; "frames"))
				If ((Value type($payload.input[$field])#Is real) & (Value type($payload.input[$field])#Is longint))
					return This._protocolError($result; "Invalid inspection field: "+$field)
				End if
			End for each
			If ($payload.input.width<1)
				return This._protocolError($result; "Invalid inspection width")
			End if
			If ($payload.input.height<1)
				return This._protocolError($result; "Invalid inspection height")
			End if
			If ($payload.input.frames<1)
				return This._protocolError($result; "Invalid inspection frames")
			End if
			If (Value type($payload.input.has_alpha)#Is boolean)
				return This._protocolError($result; "Invalid inspection has_alpha")
			End if
		: ($command="convert")
			If ($payload.operation#"convert")
				return This._protocolError($result; "Unexpected operation")
			End if
			If (Value type($payload.input)#Is object)
				return This._protocolError($result; "Missing conversion input")
			End if
			If (Value type($payload.output)#Is object)
				return This._protocolError($result; "Missing conversion output")
			End if
			If (Value type($payload.output.format)#Is text)
				return This._protocolError($result; "Invalid output format")
			End if
			If (Value type($payload.output.mode)#Is text)
				return This._protocolError($result; "Invalid output mode")
			End if
			If ((Value type($payload.output.bytes)#Is real) & (Value type($payload.output.bytes)#Is longint))
				return This._protocolError($result; "Invalid output bytes")
			End if
			If ($payload.output.bytes<=0)
				return This._protocolError($result; "Nonpositive output bytes")
			End if
		Else
			return This._protocolError($result; "Unknown response operation")
	End case
	$result.value:=$payload
	$decoded:=$result

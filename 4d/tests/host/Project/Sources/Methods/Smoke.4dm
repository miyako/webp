{"compatibilityVersion":2060}
//%attributes = {"invisible":true}
var $ignored : Integer
var $parameter; $name; $target; $format : Text
var $inputHash : Text
var $bytes : Blob
var $request; $result; $report; $options : Object
var $tool : cs.WebP.Converter
var $requestFile; $statusFile; $renamedExecutable; $input; $output : 4D.File
var $fixtures : 4D.Folder
var $invalid; $value : Variant
var HOST_StatusFile : 4D.File
var HOST_Phase : Text
var HOST_PreviousHandler : Text
var $ignored : Integer
$ignored:=Get database parameter(User param value; $parameter)
If (Is Windows)
	$requestFile:=File($parameter; fk platform path)
Else
	$requestFile:=File($parameter; fk posix path)
End if
$request:=JSON Parse($requestFile.getText())
If (Is Windows)
	$statusFile:=File($request.status; fk platform path)
	$renamedExecutable:=File($request.executable; fk platform path)
Else
	$statusFile:=File($request.status; fk posix path)
	$renamedExecutable:=File($request.executable; fk posix path)
End if
$statusFile.setText("{\"success\":false,\"phase\":\"starting\"}")
HOST_StatusFile:=$statusFile
HOST_Phase:="constructor"
HOST_PreviousHandler:=Method called on error(ek local)
ON ERR CALL("Host_error"; ek local)
$target:="macOS-universal"
If (Is Windows)
	$target:="Windows-x64"
End if
$tool:=cs.WebP.Converter.new($target)
$fixtures:=$requestFile.parent.folder("Resources")
$input:=$fixtures.file("input.png")
$bytes:=$input.getContent()
$inputHash:=Lowercase(Generate digest($bytes; SHA256 digest))
$report:=New object("success"; True; "finished"; False; "cases"; New collection)
$report.outputChecks:=New collection
$report.compiled:=$tool._cli.compiled
$report.renamedExecutable:=cs.WebP._Process.new().execute($renamedExecutable; New collection("--version"))
HOST_Phase:="feature cases"
For each ($name; New collection("webp"; "jpeg"))
	$options:=Null
	$output:=$requestFile.parent.file($name+" 日本語 ' ; & output."+($name))
	If ($name="jpeg")
		$options:=New object("to"; "jpeg"; "quality"; 80; "background"; "FFFFFF")
	End if
	$result:=$tool.convert($input; $output; $options)
	$report.cases.push($result)
	If ($result.ok)
		$bytes:=$output.getContent()
		$report.outputChecks.push((BLOB size($bytes)=$result.value.output.bytes) & ($result.value.output.format=$name))
	Else
		$report.outputChecks.push(False)
	End if
End for each
$report.version:=$tool.version()
$report.capabilities:=$tool.capabilities()
$report.inspection:=$tool.inspect($input)
$report.fileArgument:=cs.WebP._CLI.new("webp-convert"; $target).execute(New collection("inspect"; $input; "--json"))
$report.malformed:=$tool.inspect($fixtures.file("corrupt.png"))
$report.missingInput:=$tool.inspect("/tmp/webp-4d-host /missing 日本語.png")
$report.limit:=$tool.convert($input; $requestFile.parent.file("limit.webp"); New object("max-input-bytes"; 10))
$output:=$requestFile.parent.file("clobber.webp")
$report.firstWrite:=$tool.convert($input; $output; Null)
$report.noClobber:=$tool.convert($input; $output; Null)
$report.overwrite:=$tool.convert($input; $output; New object("overwrite"; True))
$report.missingBackground:=$tool.convert($input; $requestFile.parent.file("alpha.jpeg"); Null)
$options:=New object("format"; "webp"; "method"; 4; "lossless"; True; "overwrite"; True; "max-pixels-mp"; "100"; "max-frames"; "64"; "timeout"; "25"; "metadata"; "all"; "orient"; "auto")
$output:=$requestFile.parent.file("options 日本語 \" quote.webp")
$report.options:=$tool.convert($input; $output; $options)
$report.badOption:=$tool.convert($input; $requestFile.parent.file("bad.webp"); New object("fidelity"; "approximate"))
$report.badType:=$tool.convert($input; $requestFile.parent.file("bad.webp"); New object("quality"; True))
$report.badFlag:=$tool.convert($input; $requestFile.parent.file("bad.webp"); New object("lossless"; "yes"))
$report.badNumber:=$tool.convert($input; $requestFile.parent.file("bad.webp"); New object("method"; 1.5))
$report.rejectedArguments:=New collection
$report.rejectedInputs:=New collection
$report.rejectedOutputs:=New collection
$report.rejectedOptionValues:=New collection
HOST_Phase:="scalar arguments"
For each ($invalid; New collection(123; 1.25; True; False; Null; Current date; Current time; New collection; New object))
	$result:=cs.WebP._Process.new().execute($requestFile; New collection($invalid))
	$report.rejectedArguments.push(New object("type"; Value type($invalid); "result"; $result))
End for each
For each ($invalid; New collection(123; True; False; Null; Current date; Current time; New collection; New object))
	$report.rejectedInputs.push($tool._arguments($invalid; "/tmp/webp4d-host-out.webp"; Null))
End for each
For each ($invalid; New collection(123; True; False; Null; Current date; Current time; New collection; New object))
	$report.rejectedOutputs.push($tool._arguments($input; $invalid; Null))
End for each
For each ($invalid; New collection(123; True; "text"; New collection))
	$report.rejectedOptionValues.push($tool._arguments($input; "/tmp/webp4d-host-out.webp"; $invalid))
End for each
$report.rejectedNul:=New collection
HOST_Phase:="NUL arguments"
For each ($name; New collection(Char(0)+"prefix"; "middle"+Char(0)+"suffix"; "trailing"+Char(0)))
	$report.rejectedNul.push(cs.WebP._Process.new().execute($requestFile; New collection($name)))
End for each
$report.missingFile:=cs.WebP._Process.new().execute(Null; New collection)
$report.missingArguments:=cs.WebP._Process.new().execute($requestFile; Null)
HOST_Phase:="Folder argument"
$report.folderArgument:=cs.WebP._CLI.new("webp-convert"; $target).execute(New collection("inspect"; $requestFile.parent))
$folderPath:=$requestFile.parent.path
If (Is Windows)
	$folderPath:=$requestFile.parent.platformPath
End if
$name:=$report.folderArgument.stderr
$report.folderRoundTrip:=Position($folderPath; $name; *)>0
HOST_Phase:="missing executable"
$report.missing:=cs.WebP._CLI.new("missing-cli"; $target).execute(New collection("--version"))
$report.unsupported:=cs.WebP.Converter.new("unsupported-arm-target").version()
HOST_Phase:="launch failure"
$report.launch:=cs.WebP._Process.new().execute($requestFile; New collection)
HOST_Phase:="malformed protocol"
$report.protocol:=$tool._decode(New object("ok"; True; "stdout"; "not json\n"; "stderr"; ""; "exitCode"; 0); "version")
$report.protocolTypes:=New collection
For each ($name; New collection("{\"protocol\":true,\"version\":\"invalid\"}\n"; "{\"protocol\":false,\"version\":\"invalid\"}\n"))
	$report.protocolTypes.push($tool._decode(New object("ok"; True; "stdout"; $name; "stderr"; ""; "exitCode"; 0); "version"))
End for each
$report.protocolNull:=$tool._decode(New object("ok"; True; "stdout"; "{\"protocol\":null,\"version\":\"invalid\"}\n"; "stderr"; ""; "exitCode"; 0); "version")
$report.protocolScalars:=New collection
For each ($name; New collection("true\n"; "false\n"; "null\n"; "1\n"; "[]\n"))
	$report.protocolScalars.push($tool._decode(New object("ok"; True; "stdout"; $name; "stderr"; ""; "exitCode"; 0); "capabilities"))
End for each
$report.protocolFailure:=$tool._decode(New object("ok"; False; "error"; New object("code"; "cli_error"); "stdout"; "{\"protocol\":1,\"ok\":false,\"operation\":\"convert\",\"error\":{\"code\":\"output_error\",\"message\":\"x\"}}\n"; "stderr"; ""; "exitCode"; 3); "convert")
$report.preservedError:=$tool._decode(New object("ok"; False; "error"; New object("code"; "cli_error"); "stdout"; "{\"protocol\":1,\"ok\":false,\"operation\":\"convert\",\"error\":{\"code\":\"output_error\",\"message\":\"x\",\"future\":\"retained\"}}\n"; "stderr"; ""; "exitCode"; 5); "convert")
$report.disagreeError:=$tool._decode(New object("ok"; False; "error"; New object("code"; "cli_error"); "stdout"; "{\"protocol\":1,\"ok\":false,\"operation\":\"convert\",\"error\":{\"code\":\"corrupt_input\",\"message\":\"x\"}}\n"; "stderr"; ""; "exitCode"; 5); "convert")
$report.stderrFailure:=$tool._decode(New object("ok"; False; "error"; New object("code"; "cli_error"); "stdout"; "{\"protocol\":1,\"ok\":false,\"operation\":\"convert\",\"error\":{\"code\":\"output_error\",\"message\":\"x\"}}\n"; "stderr"; "noise"; "exitCode"; 5); "convert")
$report.usageError:=$tool._decode(New object("ok"; False; "error"; New object("code"; "cli_error"); "stdout"; ""; "stderr"; "error: unexpected argument '--bogus' found\n"; "exitCode"; 2); "convert")
$report.terminated:=$tool._decode(New object("ok"; False; "stdout"; ""; "stderr"; ""; "error"; New object("code"; "process_terminated"; "message"; "timeout")); "convert")
HOST_Phase:="version text tamper"
$report.versionText:=New collection
For each ($name; New collection("webp-convert\n"; "webp-convert abc.bogus\n"; "webp-convert 1.0.0\nextra\n"; "not-webp 1.0.0\n"; "{\"version\":\"1.0.0\"}\n"))
	$report.versionText.push($tool._decode(New object("ok"; True; "stdout"; $name; "stderr"; ""; "exitCode"; 0); "version"))
End for each
$report.versionGood:=$tool._decode(New object("ok"; True; "stdout"; "webp-convert 1.0.0\n"; "stderr"; ""; "exitCode"; 0); "version")
$report.versionDirty:=$tool._decode(New object("ok"; True; "stdout"; "webp-convert 1.0.0\n"; "stderr"; "x"; "exitCode"; 0); "version")
$report.versionExit:=$tool._decode(New object("ok"; True; "stdout"; "webp-convert 1.0.0\n"; "stderr"; ""; "exitCode"; 1); "version")
HOST_Phase:="File argument"
$bytes:=$input.getContent()
$report.inputPreserved:=Lowercase(Generate digest($bytes; SHA256 digest))=$inputHash
HOST_Phase:="asset checks"
$report.assetMissing:=$tool._checkAsset($renamedExecutable; $fixtures.file("missing-asset-metadata.json"))
$report.assetMismatch:=$tool._checkAsset($requestFile; $tool._cli.resolve().file.parent.parent.parent.file("webp-assets.json"))
$report.finished:=True
ON ERR CALL(HOST_PreviousHandler; ek local)
$statusFile.setText(JSON Stringify($report))
HOST_StatusFile:=Null
HOST_Phase:=""
HOST_PreviousHandler:=""
QUIT 4D

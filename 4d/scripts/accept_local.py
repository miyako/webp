#!/usr/bin/env python3
"""Local acceptance for the webp 4D component (ADR-012 two-track policy).

Steps: CLI asset verify -> host test (source) -> tool4d compile ->
package (dist/webp.zip) -> compiled host test -> stability recheck.
Hosted/native-intel-4D/Windows/notarization coverage is always reported
not_run: local acceptance is labeled partial by construction.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import PipelineError, config, no_symlinks, sha256
from stage_cli import verify as verify_assets, stage as stage_assets
from compile_component import compile_component
from package_component import package, unpack_for_test
from test_4d import test_4d

ROOT = Path(__file__).resolve().parents[1]


def accept(root, tool4d):
    root = Path(root).resolve()
    output = root / "build" / "acceptance.json"
    no_symlinks(output, recursive=False)
    output.parent.mkdir(parents=True, exist_ok=True)
    names = ["cli_assets", "source_host", "compile", "package", "compiled_host",
             "hosted_cli", "native_intel_4d", "windows_4d", "release_download", "signing"]
    report = {"schema": 1, "scope": "local", "release_ready": False, "result": "partial",
              "started_at": datetime.now(timezone.utc).isoformat(),
              "finished": False, "steps": {name: {"status": "not_run", "reason": "not exercised"}
                                           for name in names}}

    def save():
        output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    def step(name, action):
        report["steps"][name] = {"status": "failed", "reason": "step did not complete"}
        save()
        value = action()
        report["steps"][name] = {"status": "passed"}
        if isinstance(value, dict):
            report["steps"][name]["details"] = value
        else:
            report["steps"][name]["details"] = {"result": str(value)}
        save()
        return value

    save()
    try:
        report["configuration"] = config(root)
        report["doctor"] = {
            "tool4d": {"bin": str(Path(tool4d).resolve()),
                       "sha256": sha256(Path(tool4d).resolve()),
                       "version": subprocess.run(
                           [str(Path(tool4d).resolve()), "--version"],
                           check=True, capture_output=True, text=True, timeout=60).stdout.strip()},
            "platform": sys.platform,
        }
        save()
        lock = step("cli_assets", lambda: stage_assets(root) and verify_assets(root))
        source = step("source_host", lambda: test_4d(root, tool4d))
        (output.parent / "source-host-status.json").write_text(
            json.dumps(source, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        step("compile", lambda: compile_component(root, tool4d))
        artifact = step("package", lambda: package(root, tag=config(root)["version"]))
        report["artifact"] = {"path": str(artifact), "sha256": sha256(artifact),
                             "bytes": Path(artifact).stat().st_size}
        unpacked = unpack_for_test(root, artifact)
        compiled = step("compiled_host", lambda: test_4d(root, tool4d, unpacked))
        (output.parent / "compiled-host-status.json").write_text(
            json.dumps(compiled, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        if sha256(artifact) != report["artifact"]["sha256"]:
            raise PipelineError("final archive changed during host acceptance")
        shutil.rmtree((root / "build" / "release-test"))
        report["finished"] = True
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        report["reason"] = ("local package verified; hosted runner, native Intel 4D, Windows 4D, "
                            "release download and Apple signing remain separate observations")
        save()
        evidence = artifact.with_suffix(".acceptance.json")
        no_symlinks(evidence, recursive=False)
        evidence.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        return report
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        report.update(result="failed", finished=False, error=str(error))
        save()
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--tool4d", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = accept(args.root, args.tool4d)
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.exit(2, f"local acceptance failed (see {ROOT}/build/acceptance.json): {error}\n")
    print(json.dumps({"result": report["result"], "release_ready": False,
                      "artifact": report["artifact"]}, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())

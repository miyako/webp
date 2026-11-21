#!/usr/bin/env python3
"""Hosted acceptance for the webp 4D component (quickdraw-parity, ADR-016).

The local pipeline replayed on a GitHub macOS runner, plus the hosted-only
steps: pinned CLI asset download, optional Apple signing + notarization of
the compiled archive, and a final compiled-host run against the exact
published bytes. Native Intel 4D, Windows 4D and the installed-release
download check remain separate observations and stay `not_run`; the
report-level `release_ready` therefore stays false by construction.

Steps (in order): cli_assets -> source_host -> compile -> package ->
[unsigned_compiled_host when signing] -> [signing] -> [signing_proof] ->
compiled_host (on the final archive) -> hosted_cli -> stability recheck.

Requires: macOS ARM64 host, GITHUB_ACTIONS=true, and a --repository value
matching the asset lock. --require-apple-signing can only upgrade the
release-policy decision, never downgrade it. Evidence is written to
dist/<component>.acceptance.json; the reports live in build/.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import PipelineError, config, load_lock, no_symlinks, regular, sha256
from compile_component import compile_component
from package_component import package, unpack_for_test
from stage_cli import verify as verify_assets, stage as stage_assets
from test_4d import test_4d

ROOT = Path(__file__).resolve().parents[1]


def git(arguments, cwd):
    result = subprocess.run(["git", *arguments], cwd=cwd, capture_output=True,
                            text=True, encoding="utf-8", check=False)
    if result.returncode:
        raise PipelineError("git " + arguments[0] + " failed: " + result.stderr.strip())
    return result.stdout.strip()


def platform_machine():
    try:
        return subprocess.run(["uname", "-m"], capture_output=True, text=True,
                              check=True, timeout=30).stdout.strip().lower()
    except (OSError, subprocess.SubprocessError) as error:
        raise PipelineError("cannot determine host machine: " + str(error)) from None


def require_hosted_env(root, repository):
    """Gate the hosted context before any mutation; returns the checkout SHA."""
    if platform.system() != "Darwin" or platform_machine() not in {"arm64", "aarch64"}:
        raise PipelineError("hosted acceptance requires an Apple Silicon macOS runner")
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise PipelineError("hosted acceptance requires GITHUB_ACTIONS=true")
    if not repository or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_-]+",
                                          repository, re.ASCII):
        raise PipelineError("hosted acceptance requires --repository owner/name")
    lock = load_lock(root)
    if repository != lock["repository"]:
        raise PipelineError("--repository does not match the asset lock: " + lock["repository"])
    commit = git(["rev-parse", "HEAD"], root.parent)
    if not re.fullmatch(r"[0-9a-f]{40}", commit, re.ASCII):
        raise PipelineError("hosted acceptance requires a real git checkout")
    return commit


def fetch_pinned_cli_assets(root):
    """Download any lock asset missing locally, verifying bytes against the lock.

    The lock is authoritative and tamper-evident: a downloaded archive is
    accepted only when its SHA-256 matches the lock. cli/assets is
    gitignored; it is pure cache and stays that way.
    """
    lock = load_lock(root)
    for _, entry in lock["assets"].items():
        archive = root / entry["local_path"]
        if archive.is_file():
            continue
        if any(path.is_symlink() for path in [archive, *archive.parents]):
            raise PipelineError("asset cache path must not traverse symlinks: " + str(archive))
        staging = archive.with_name(archive.name + ".downloading")
        staging.parent.mkdir(parents=True, exist_ok=True)
        try:
            with urllib.request.urlopen(entry["url"], timeout=120) as response, \
                    staging.open("wb") as output:
                while chunk := response.read(1024 * 1024):
                    output.write(chunk)
            if sha256(staging) != entry["sha256"]:
                raise PipelineError("downloaded CLI asset digest differs from the lock: "
                                    + entry["url"])
            staging.replace(archive)
        except (OSError, PipelineError) as error:
            staging.unlink(missing_ok=True)
            if isinstance(error, PipelineError):
                raise
            raise PipelineError("pinned CLI asset download failed: " + str(error)) from None
        finally:
            staging.unlink(missing_ok=True)
    return lock


def run_final_cli_smoke(root, unpacked):
    """Prove the preinstalled CLI inside the accepted archive runs here."""
    cli = unpacked / "Resources" / "bin" / "macOS" / config(root)["cli"]
    subprocess.run([str(regular(cli)), "--version"], check=True,
                   capture_output=True, timeout=60)
    return "preinstalled CLI runs on the hosted macOS runner"


def signing_proof(root, cfg, final_sha):
    from sign_release import submission_uuid
    proof_file = root / "dist" / (cfg["repository"] + "-signing.json")
    proof = json.loads(regular(proof_file).read_text(encoding="utf-8"))
    if (not isinstance(proof, dict) or proof.get("schema") != 1
            or proof.get("version") != cfg["version"]
            or not isinstance(proof.get("zip"), dict)
            or proof["zip"].get("sha256") != final_sha
            or proof.get("hostValidation") != "required-after-signing"):
        raise PipelineError("signing proof does not match the final archive")
    submission_uuid(proof.get("notarization", {}).get("submissionUUID"))
    return {"sha256": final_sha, "notarizedViaDMG": proof["zip"].get("notarizedViaDMG") is True}


def signed_archive_paths(staged, cfg):
    """Mach-O files inside the staged component that Apple signing covers."""
    from sign_release import macho_architectures, SigningError
    signed = [staged / "Resources" / "bin" / "macOS" / cfg["cli"]]
    for path in sorted((staged / "Libraries").rglob("*")):
        if not path.is_file() or path.is_symlink():
            continue
        try:
            macho_architectures(path)
            signed.append(path)
        except SigningError:
            if path.suffix in {".dylib", ".so"}:
                raise
    return signed


def accept(root, tool4d, *, repository=None, require_apple_signing=False):
    root = Path(root).resolve()
    cfg = config(root)
    commit = require_hosted_env(root, repository)
    output = root / "build" / "hosted-acceptance.json"
    no_symlinks(output, recursive=False)
    output.parent.mkdir(parents=True, exist_ok=True)
    names = ["cli_assets", "source_host", "compile", "package", "unsigned_compiled_host",
             "signing", "signing_proof", "compiled_host", "hosted_cli",
             "native_intel_4d", "windows_4d", "release_download"]
    report = {"schema": 1, "scope": "hosted", "repository": repository,
              "source_sha256": commit, "release_ready": False, "result": "partial",
              "started_at": datetime.now(timezone.utc).isoformat(),
              "finished": False,
              "steps": {name: {"status": "not_run", "reason": "not exercised"} for name in names}}

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
    dist = root / "dist"
    final_zip = dist / (cfg["repository"] + ".zip")
    try:
        report["configuration"] = cfg
        report["doctor"] = {
            "tool4d": {"bin": str(Path(tool4d).resolve()),
                       "sha256": sha256(regular(tool4d)),
                       "version": subprocess.run([str(regular(tool4d)), "--version"],
                                                 check=True, capture_output=True, text=True,
                                                 timeout=60).stdout.strip()},
            "platform": sys.platform,
        }
        save()

        def cli_assets():
            fetch_pinned_cli_assets(root)
            stage_assets(root)
            return verify_assets(root)
        step("cli_assets", cli_assets)
        source = step("source_host", lambda: test_4d(root, tool4d))
        (output.parent / "source-host-status.json").write_text(
            json.dumps(source, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        step("compile", lambda: compile_component(root, tool4d))

        from sign_release import release_policy
        signing = bool(release_policy(root)) or require_apple_signing
        artifact = step("package", lambda: package(root, tag=cfg["version"]))
        # quickdraw parity: evidence paths are root-relative POSIX strings,
        # never absolute — the verifier may run on a different runner OS.
        report["artifact"] = {"path": "dist/" + artifact.name,
                              "sha256": sha256(artifact),
                              "bytes": Path(artifact).stat().st_size, "signed": signing}
        unsigned_hash = report["artifact"]["sha256"]
        save()

        if signing:
            unpacked = unpack_for_test(root, artifact)
            step("unsigned_compiled_host", lambda: test_4d(root, tool4d, unpacked))
            proof = step("signing", lambda: sign_and_return_proof(root, artifact))
            report["steps"]["signing"]["details"] = {
                "notarization_status": proof["notarization"]["status"],
                "signed_files": [entry["path"] for entry in proof["signedFiles"]]}
            if final_zip.resolve() != Path(artifact).resolve():
                raise PipelineError("signing did not replace the published archive in place")
            report["unsigned_artifact"] = {"path": "dist/" + artifact.name, "sha256": unsigned_hash}
            report["artifact"]["sha256"] = sha256(regular(final_zip))
            report["artifact"]["bytes"] = final_zip.stat().st_size
            if report["artifact"]["sha256"] == unsigned_hash:
                raise PipelineError("signing left the archive byte-identical to the unsigned build")
            save()
            step("signing_proof", lambda: signing_proof(root, cfg, report["artifact"]["sha256"]))
        else:
            report["steps"]["signing"] = {"status": "not_run", "reason": "policy: unsigned release"}
            report["steps"]["signing_proof"] = {"status": "not_run", "reason": "policy: unsigned release"}
            save()

        selected = regular(final_zip)
        compiled = step("compiled_host", lambda: test_4d(root, tool4d,
                                                         unpack_for_test(root, selected)))
        (output.parent / "compiled-host-status.json").write_text(
            json.dumps(compiled, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        step("hosted_cli", lambda: run_final_cli_smoke(
            root, root / "build" / "release-test" / (cfg["repository"] + ".4dbase")))
        if sha256(selected) != report["artifact"]["sha256"]:
            raise PipelineError("final archive changed during host acceptance")
        shutil.rmtree(root / "build" / "release-test")
        report["finished"] = True
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        report["reason"] = ("hosted package verified on the pinned tool4d; native Intel 4D, "
                            "Windows 4D and the installed-release download check remain "
                            "separate observations")
        save()
        evidence = dist / (cfg["repository"] + ".acceptance.json")
        no_symlinks(evidence, recursive=False)
        evidence.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        return report
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        report.update(result="failed", finished=False, error=str(error))
        save()
        raise


def sign_and_return_proof(root, artifact):
    """Run Apple signing; sign_release replaces dist/<component>.zip in place."""
    import sign_release
    proof = sign_release.sign_release(root)
    final_zip = root / "dist" / (config(root)["repository"] + ".zip")
    if Path(artifact).resolve() != final_zip.resolve():
        raise PipelineError("signing did not target the packaged release artifact")
    return proof


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tool4d", type=Path,
                        default=ROOT / ".tools" / "tool4d.app" / "Contents" / "MacOS" / "tool4d")
    parser.add_argument("--require-apple-signing", action="store_true",
                        help="require signing in addition to version policy; never permits a downgrade")
    args = parser.parse_args()
    try:
        report = accept(args.root, args.tool4d, repository=args.repository,
                        require_apple_signing=args.require_apple_signing)
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.exit(2, f"hosted acceptance failed (see {ROOT}/build/hosted-acceptance.json): {error}\n")
    print(json.dumps({"result": report["result"], "release_ready": report["release_ready"],
                      "artifact": report["artifact"]}, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())

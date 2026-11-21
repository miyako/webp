#!/usr/bin/env python3
"""Build + verify the pinned CLI asset pack for the 4D component (ADR-012).

Modes:
  build   Package the built universal binary + release manifest into
          4d/cli/assets/webp-convert-<version>-<target>.zip, verify every
          hash and architecture, then (re)write 4d/cli/asset-lock.json.
  stage   Verify the lock against the local asset ZIPs and stage the CLI
          into the component Resources (bin + licenses + webp-assets.json).
  verify  Verify only (lock shape, ZIP hashes, digests, Mach-O slices).

The lock always pins GitHub release URLs of the CLI's own v-prefixed tag
(see ADR-012); component releases bundle the staged, digest-verified
binary under Resources/bin. Windows has no CLI release yet: the lock and
the component deliberately have no windows entry, and Windows installs
fail asset verification instead of running unverified code.
"""
import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import (PipelineError, check_universal_macho, cli_version, commit,
                    load_lock, no_symlinks, read_json_file, regular, safe_unpack_zip,
                    sha256, write_zip)

ROOT = Path(__file__).resolve().parents[1]


def universal_binary(root):
    candidate = repo_binary(root)
    data = candidate.read_bytes()
    check_universal_macho(data)
    return candidate


def repo_binary(root):
    webp_root = root.parent
    candidate = webp_root / "target" / "universal" / "webp-convert"
    if candidate.is_file() and not candidate.is_symlink():
        return candidate
    raise PipelineError("universal CLI binary is missing; run scripts/build-macos-universal.sh first: "
                        + str(candidate))


def build(root, target="universal2-apple-darwin"):
    root = Path(root).resolve()
    webp_root = root.parent
    binary = universal_binary(root)
    version = cli_version(binary)
    revision = commit(webp_root)
    manifests = webp_root / "manifests"
    matches = sorted(manifests.glob(f"webp-convert-{version}-{target}*.manifest.json"))
    if len(matches) != 1:
        raise PipelineError("expected exactly one release manifest for " + version
                            + "/" + target + " in " + str(manifests) + "; found "
                            + str(len(matches)))
    manifest = regular(matches[0])
    manifest_sha256 = sha256(manifest)
    if manifest_sha256 != sha256(manifest):
        raise PipelineError("manifest read instability")
    lock = read_json_file(root / "cli" / "asset-lock.json") \
        if (root / "cli" / "asset-lock.json").is_file() else {}
    if lock.get("version") not in (None, version):
        raise PipelineError("asset-lock already pins a different CLI version: "
                            + str(lock.get("version")))
    assets = root / "cli" / "assets"
    assets.mkdir(parents=True, exist_ok=True)
    zip_name = f"webp-convert-{version}-{target}.zip"
    entries = [
        ("webp-convert", binary.read_bytes(), 0o755),
        ("manifest.json", manifest.read_bytes(), 0o644),
    ]
    destination = assets / zip_name
    no_symlinks(destination, recursive=False)
    write_zip(destination, entries)
    digest = sha256(destination)
    # verify round-trip: unpack to a temp dir, digest the binary, smoke it
    with tempfile.TemporaryDirectory(prefix="webp-asset-") as workspace:
        unpacked = safe_unpack_zip(destination, Path(workspace) / "asset", ("webp-convert",))
        extracted = regular(unpacked / "webp-convert")
        binary_sha256 = sha256(extracted)
        if binary_sha256 != sha256(binary):
            raise PipelineError("asset ZIP round-trip changed the CLI binary")
        subprocess.run([str(extracted), "--version"], check=True, capture_output=True, timeout=60)
    lock = {
        "schema": 1,
        "repository": "miyako/webp",
        "version": version,
        "tag": "v" + version,
        "source_revision": revision,
        "assets": {
            "macos": {
                "url": f"https://github.com/miyako/webp/releases/download/v{version}/{zip_name}",
                "target": target,
                "sha256": digest,
                "manifest_sha256": manifest_sha256,
                "binary_sha256": binary_sha256,
                "local_path": "cli/assets/" + zip_name,
                "local_manifest": manifest.relative_to(webp_root).as_posix(),
            }
        },
    }
    (root / "cli" / "asset-lock.json").write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")
    return lock


def verify(root):
    root = Path(root).resolve()
    lock = load_lock(root)
    for platform, entry in lock["assets"].items():
        local = entry.get("local_path")
        if not local:
            raise PipelineError("asset-lock " + platform + " has no local_path to verify against")
        archive = root / local
        if not archive.is_file():
            raise PipelineError("pinned CLI asset is missing; run build first: " + str(archive))
        if sha256(archive) != entry["sha256"]:
            raise PipelineError("pinned CLI asset digest mismatch: " + str(archive))
        with tempfile.TemporaryDirectory(prefix="webp-verify-") as workspace:
            unpacked = safe_unpack_zip(
                archive, Path(workspace) / platform, ("webp-convert", "webp-convert.exe"))
            binary = unpacked / ("webp-convert.exe" if platform == "windows" else "webp-convert")
            data = regular(binary).read_bytes()
            if platform == "macos":
                check_universal_macho(data)
            hashes = {"binary": sha256(binary)}
            manifest = unpacked / "manifest.json"
            if manifest.is_file():
                if sha256(manifest) != entry["manifest_sha256"]:
                    raise PipelineError("pinned manifest digest mismatch: " + str(archive))
                hashes["manifest"] = sha256(manifest)
                recorded = read_json_file(manifest).get("commit")
                if recorded not in (None, lock["source_revision"]):
                    raise PipelineError("pinned manifest commit differs from the lock "
                                        + lock["source_revision"] + ": " + str(recorded))
            if hashes.get("binary") != entry["binary_sha256"]:
                raise PipelineError("pinned binary digest mismatch: " + str(archive))
            subprocess.run([str(binary), "--version"], check=True, capture_output=True, timeout=60)
    return lock


def stage(root):
    root = Path(root).resolve()
    lock = verify(root)
    component = root / "component" / read_json_file(root / "template.json")["repository"]
    resources = component / "Resources"
    (resources / "bin" / "macOS").mkdir(parents=True, exist_ok=True)
    for platform, entry in lock["assets"].items():
        archive = root / entry["local_path"]
        folder = {"macos": "macOS", "windows": "Windows"}[platform]
        name = "webp-convert" + (".exe" if platform == "windows" else "")
        destination = resources / "bin" / folder / name
        with tempfile.TemporaryDirectory(prefix="webp-stage-") as workspace:
            unpacked = safe_unpack_zip(
                archive, Path(workspace) / platform, (name,))
            binary = regular(unpacked / name)
            if sha256(binary) != entry["binary_sha256"]:
                raise PipelineError("staged binary digest mismatch: " + str(archive))
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(binary, destination)
            destination.chmod(0o755)
            manifest = resources / "licenses" / "webp-convert" / platform
            if manifest.exists():
                shutil.rmtree(manifest)
            shutil.copytree(unpacked, manifest, ignore=shutil.ignore_patterns("webp-convert", "*.exe"))
    regular(resources / "bin" / "macOS" / "webp-convert")
    (resources / "webp-assets.json").write_text(
        json.dumps(public_lock(lock), indent=2) + "\n", encoding="utf-8")
    return resources


def public_lock(lock):
    """The runtime-visible provenance: no local staging paths."""
    public = {key: lock[key] for key in ("schema", "repository", "version", "tag", "source_revision")}
    public["assets"] = {
        platform: {key: entry[key] for key in ("url", "target", "sha256", "manifest_sha256", "binary_sha256")}
        for platform, entry in lock["assets"].items()
    }
    return public


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("build", "stage", "verify"))
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    try:
        if args.mode == "build":
            lock = build(args.root)
            print(json.dumps({"version": lock["version"], "tag": lock["tag"],
                              "revision": lock["source_revision"]}, indent=2))
        elif args.mode == "verify":
            print(json.dumps(public_lock(verify(args.root)), indent=2))
        else:
            print("staged:", stage(args.root))
    except PipelineError as error:
        print("CLI asset pipeline failed: " + str(error), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())

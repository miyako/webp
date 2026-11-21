#!/usr/bin/env python3
"""Package the compiled component as dist/webp.zip (repository basename asset).

This is an unsigned dependency-manager archive, not a notarized application.
Project metadata and compiled code are retained; interpreted classes/methods
are excluded. The pinned CLI is preinstalled under Resources/bin. Both ZIP
levels are deterministic and require verified compilation + staged CLI assets.

Port of the audited quickdraw packager; platform set follows the components
asset-lock (Windows is absent until a verified Windows CLI release exists).
"""
import argparse
import io
import json
import os
import re
import shutil
import stat
import struct
import sys
from pathlib import Path, PurePosixPath
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import (PipelineError, check_universal_macho, config, load_lock,
                    no_symlinks, read_json_file, regular, sha256, write_zip)

ROOT = Path(__file__).resolve().parents[1]
COMPILE_TARGETS = {"x86_64_generic", "arm64_macOS_lib"}
EXCLUDED = {"data", "logs", ".git", ".ds_store", "__macosx"}


def validate_resources(root, component, cli):
    lock = load_lock(root)
    mac = regular(component / "Resources" / "bin" / "macOS" / cli)
    check_universal_macho(mac.read_bytes())
    binary_sha256 = sha256(mac)
    entry = lock["assets"]["macos"]
    if binary_sha256 != entry["binary_sha256"]:
        raise PipelineError("staged macOS CLI binary hash differs from the release lock")
    windows_path = component / "Resources" / "bin" / "Windows" / (cli + ".exe")
    if "windows" in lock["assets"]:
        check = regular(windows_path)
        data = check.read_bytes()
        if len(data) < 64 or data[:2] != b"MZ":
            raise PipelineError("Windows resource must be a PE executable")
        if sha256(check) != lock["assets"]["windows"]["binary_sha256"]:
            raise PipelineError("staged Windows CLI binary hash differs from the release lock")
    elif windows_path.exists():
        raise PipelineError("lock has no windows asset but Resources/bin/Windows exists")
    staged = read_json_file(component / "Resources" / "webp-assets.json")
    public = {key: lock[key] for key in ("schema", "repository", "version", "tag", "source_revision")}
    public["assets"] = {
        platform: {key: item[key] for key in
                   ("url", "target", "sha256", "manifest_sha256", "binary_sha256")}
        for platform, item in lock["assets"].items()
    }
    if staged != public:
        raise PipelineError("staged provenance (Resources/webp-assets.json) differs from the release lock")
    licenses = component / "Resources" / "licenses"
    if not licenses.is_dir() or not any(regular(p).stat().st_size for p in licenses.rglob("*")
                                        if p.is_file()):
        raise PipelineError("runtime license notices are missing")
    return mac, lock


def validate_compile(root, component, repository):
    marker = root / "build" / "compile-status.json"
    if not marker.is_file() or marker.is_symlink():
        raise PipelineError("successful compile-status.json is required; source-only releases are prohibited")
    status = read_json_file(marker)
    project = component / "Project" / (repository + ".4DProject")
    if (status.get("success") is not True or not isinstance(status.get("targets"), list)
            or set(status["targets"]) != COMPILE_TARGETS):
        raise PipelineError("compile marker must confirm success for both required targets")
    marked_project = status.get("project")
    if not isinstance(marked_project, str):
        raise PipelineError("compile marker must identify the project path")
    candidate = Path(marked_project)
    if not candidate.is_absolute():
        candidate = root / candidate
    if candidate.resolve() != project.resolve():
        raise PipelineError("compile marker project does not match configured project")
    from compile_component import input_digest, output_digest
    if status.get("input_sha256") != input_digest(root):
        raise PipelineError("compile marker input_sha256 does not match current component inputs; recompile")
    if not project.is_file():
        raise PipelineError("configured .4DProject is missing")
    compiled = project.parent / "DerivedData" / "CompiledCode"
    required = [
        component / "Libraries" / "lib4d-arm64.dylib",
        compiled / "classes.json", compiled / "map.json",
    ]
    for path in required:
        if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
            raise PipelineError(f"required compiled artifact is missing or empty: {path}")
    objects = [path for path in compiled.iterdir()
               if re.fullmatch(r"MX64 [0-9]+", path.name) and path.is_file()]
    if not objects or any(path.is_symlink() or path.stat().st_size == 0 for path in objects):
        raise PipelineError("CompiledCode must contain nonempty regular MX64 numeric-id files")
    if status.get("output_sha256") != output_digest(component):
        raise PipelineError("compile marker output_sha256 does not match compiled artifacts; recompile")


def include_project(relative):
    parts = relative.parts
    if len(parts) < 2 or parts[0] != "Project" or relative.suffix.lower() == ".4dm":
        return False
    if len(parts) == 2:
        return relative.suffix == ".4DProject"
    if parts[1] == "Sources":
        return not any(part.lower() in {"classes", "methods"} for part in parts[2:])
    if parts[1] == "DerivedData":
        return (parts[2] == "CompiledCode"
                or len(parts) == 3 and parts[2] in {
                    "methodAttributes.json", "formAttributes.json", "webservices.json"})
    return False


def package(root=ROOT, *, repository=None, tag=None):
    root = Path(root).resolve()
    tag = tag if tag is not None else os.environ.get("GITHUB_REF_NAME")
    if tag is None:
        raise PipelineError("a prefix-free SemVer release tag is required (e.g. 20.0.0)")
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
        raise PipelineError("release tag must be prefix-free SemVer without v: " + tag)
    config_data = config(root)
    if tag != config_data["version"]:
        raise PipelineError("release tag must equal template.json version: "
                            + config_data["version"] + " != " + tag)
    if config_data["cli_source"] != "local-webp-v1":
        raise PipelineError("unexpected cli_source")
    component = root / "component" / config_data["repository"]
    no_symlinks(component)
    validate_resources(root, component, config_data["cli"])
    validate_compile(root, component, config_data["repository"])
    license_path = root.parent / "LICENSE"
    if not license_path.is_file():
        raise PipelineError("repository LICENSE is required")
    files = []
    for path in sorted(component.rglob("*")):
        relative = path.relative_to(component)
        if any(part.lower() in {".tools", "tool4d.app"} for part in relative.parts):
            raise PipelineError("vendor tooling/local installation must not be included in component packages")
        if any(part.lower() in EXCLUDED for part in relative.parts):
            continue
        if ("deriveddata" in (part.lower() for part in relative.parts)
                and relative.parts[:2] != ("Project", "DerivedData")):
            continue
        if path.is_symlink():
            raise PipelineError(f"symlinks are prohibited in component packages: {relative}")
        if any("\\" in part or ":" in part for part in relative.parts):
            raise PipelineError("unsafe archive path")
        if path.is_file():
            if include_project(relative) or relative.parts[0] in {"Resources", "Documentation", "Libraries"}:
                files.append((relative.as_posix(), path))
        elif not path.is_dir():
            raise PipelineError(f"non-regular component entry: {relative}")
    files = [(name, path) for name, path in files if name != "Resources/licenses/LICENSE"]
    files.append(("Resources/licenses/LICENSE", license_path))
    # Keep the Project/ anchor inside the standard-ZIP .4DZ, as emitted by 4D.
    project_entries = [
        (name, path.read_bytes(), 0o644)
        for name, path in files if name.startswith("Project/")
    ]
    with io.BytesIO() as packed_stream:
        write_zip(packed_stream, project_entries)
        packed_bytes = packed_stream.getvalue()
    prefix = config_data["repository"] + ".4dbase/"
    destination = root / "dist" / (config_data["repository"] + ".zip")
    if any(path.is_symlink() for path in [destination, *destination.parents]):
        raise PipelineError("package destination must not traverse symlinks")
    destination.parent.mkdir(parents=True, exist_ok=True)
    entries = [(prefix + config_data["repository"] + ".4DZ", packed_bytes, 0o644)]
    for name, path in files:
        if name.startswith("Project/"):
            continue
        executable = name in (f"Resources/bin/macOS/{config_data['cli']}",
                              f"Resources/bin/Windows/{config_data['cli']}.exe")
        library = name.startswith("Libraries/") and (path.stat().st_mode & 0o111)
        entries.append((prefix + name, path.read_bytes(), 0o755 if executable or library else 0o644))
    write_zip(destination, entries)
    return destination


def unpack_for_test(root, archive):
    """Restore only this generated .4dbase for the compiled-host test."""
    root = Path(root).resolve()
    destination_root = root / "build" / "release-test"
    destination = destination_root / "webp.4dbase"
    no_symlinks(destination_root, recursive=False)
    prefix = destination.name
    with zipfile.ZipFile(archive) as packed:
        members = packed.infolist()
        names = set()
        for entry in members:
            path = PurePosixPath(entry.filename)
            mode = entry.external_attr >> 16
            if (not path.parts or path.parts[0] != prefix or path.is_absolute()
                    or ".." in path.parts or "\\" in entry.filename or ":" in entry.filename
                    or stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)
                    or entry.filename in names):
                raise PipelineError("unsafe compiled component archive")
            names.add(entry.filename)
        if prefix + "/" + "webp.4DZ" not in names:
            raise PipelineError("compiled component archive is missing its .4DZ")
        if destination.exists():
            shutil.rmtree(destination)
        for entry in members:
            path = destination_root.joinpath(*PurePosixPath(entry.filename).parts)
            if entry.is_dir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with packed.open(entry) as source, path.open("wb") as output:
                    shutil.copyfileobj(source, output)
                path.chmod((entry.external_attr >> 16) & 0o777 or 0o644)
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--tag", help="prefix-free SemVer release tag, e.g. 20.0.0; defaults to GITHUB_REF_NAME")
    parser.add_argument("--unpack-test", action="store_true",
                        help="also unpack the generated archive under build/release-test")
    args = parser.parse_args()
    try:
        archive = package(args.root, tag=args.tag)
        print(archive if not args.unpack_test else unpack_for_test(args.root, archive))
    except PipelineError as error:
        parser.exit(2, f"component packaging failed: {error}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Shared helpers for the webp 4D component pipeline.

All staging/packaging invariants follow the audited miyako/quickdraw
external-CLI adapter: no symlinks in inputs/outputs, byte-exact archives,
verified Mach-O universal slices, and lock-consistent hashes everywhere.
"""
import hashlib
import json
import os
import re
import shutil
import stat
import struct
import subprocess
import tempfile
from pathlib import Path, PurePosixPath
import zipfile

MACHO_MAGICS = {b"\xcf\xfa\xed\xfe", b"\xc7\xfa\xed\xfe", b"\xce\xfa\xed\xfe", b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf"}
XFAT_WIDE = b"\xca\xfe\xba\xbf"
XFAT_32 = b"\xca\xfe\xba\xbe"
CPU_X86_64 = 0x01000007
CPU_ARM64 = 0x0100000C


class PipelineError(ValueError):
    pass


def read_json(text):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise PipelineError("duplicate JSON key: " + key)
            result[key] = value
        return result

    def invalid_constant(value):
        raise PipelineError("invalid JSON constant: " + value)

    return json.loads(text, object_pairs_hook=unique_object, parse_constant=invalid_constant)


def read_json_file(path):
    return read_json(Path(path).read_text(encoding="utf-8"))


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def regular(path):
    """Require a regular, non-symlink existing file."""
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise PipelineError("missing regular file: " + str(path))
    return path


def no_symlinks(path, recursive=True):
    path = Path(path).absolute()
    if any(parent.is_symlink() for parent in (path, *path.parents)):
        raise PipelineError("path must not traverse symlinks: " + str(path))
    if recursive and path.is_dir():
        for directory, folders, files in os.walk(path, followlinks=False):
            for name in folders + files:
                if (Path(directory) / name).is_symlink():
                    raise PipelineError("path tree must not contain symlinks: " + str(path))


def commit(repo_root):
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=repo_root,
                            capture_output=True, text=True, check=True, timeout=30)
    revision = result.stdout.strip()
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise PipelineError("cannot resolve a 40-hex source revision in " + str(repo_root))
    return revision


def cli_version(binary):
    result = subprocess.run([str(binary), "--version"], capture_output=True, text=True, timeout=60)
    if result.returncode != 0:
        raise PipelineError("CLI --version failed: " + result.stderr)
    match = re.fullmatch(r"webp-convert (\d+\.\d+\.\d+(?:[^\r\n]*))\n", result.stdout)
    if not match:
        raise PipelineError("CLI --version output does not match the pinned plain-text contract: "
                            + repr(result.stdout))
    return match.group(1)


def check_universal_macho(data, label="macOS resource"):
    """Exactly two slices, x86_64 + arm64, valid bounded thin-64 headers (AAPI guideline)."""
    if len(data) < 8 or data[:4] not in (XFAT_WIDE, XFAT_32):
        raise PipelineError(label + " must be a universal Mach-O")
    wide = data[:4] == XFAT_WIDE
    count = struct.unpack_from(">I", data, 4)[0]
    entry_size = 32 if wide else 20
    if count != 2 or len(data) < 8 + count * entry_size:
        raise PipelineError(label + " must contain exactly two slices")
    architectures = set()
    ranges = []
    for index in range(count):
        position = 8 + index * entry_size
        cpu = struct.unpack_from(">I", data, position)[0]
        offset, size = struct.unpack_from(">QQ" if wide else ">II", data, position + 8)
        if (offset < 8 + count * entry_size or size < 32 or offset + size > len(data)
                or data[offset:offset + 4] != b"\xcf\xfa\xed\xfe"
                or struct.unpack_from("<I", data, offset + 4)[0] != cpu):
            raise PipelineError("invalid Mach-O slice header or bounds")
        if any(offset < end and start < offset + size for start, end in ranges):
            raise PipelineError("overlapping Mach-O slices")
        ranges.append((offset, offset + size))
        architectures.add(cpu)
    if architectures != {CPU_X86_64, CPU_ARM64}:
        raise PipelineError(label + " must contain x86_64 and arm64 slices")


def check_windows_pe(data, label="Windows resource"):
    if len(data) < 64 or data[:2] != b"MZ":
        raise PipelineError(label + " must be a PE executable")
    offset = struct.unpack_from("<I", data, 0x3C)[0]
    if (offset < 64 or len(data) < offset + 26 or data[offset:offset + 4] != b"PE\0\0"
            or struct.unpack_from("<H", data, offset + 4)[0] not in (0x8664, 0xAA64)
            or struct.unpack_from("<H", data, offset + 24)[0] != 0x20B):
        raise PipelineError(label + " must have a 64-bit PE32+ header")


def safe_unpack_zip(archive, destination, executable_names=()):
    """Validate every member (absolute/traversal/dup/non-regular rejected), then extract."""
    archive = Path(archive)
    destination = Path(destination)
    with zipfile.ZipFile(archive) as packed:
        names = set()
        for entry in packed.infolist():
            path = PurePosixPath(entry.filename)
            mode = entry.external_attr >> 16
            if (not path.parts or path.is_absolute() or ".." in path.parts
                    or "\\" in entry.filename or ":" in entry.filename
                    or stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)
                    or entry.filename in names):
                raise PipelineError("unsafe archive entry: " + entry.filename)
            names.add(entry.filename)
        if destination.exists():
            shutil.rmtree(destination)
        for entry in packed.infolist():
            path = destination.joinpath(*PurePosixPath(entry.filename).parts)
            if entry.is_dir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with packed.open(entry) as source, path.open("wb") as output:
                    shutil.copyfileobj(source, output)
                mode = (entry.external_attr >> 16) & 0o777
                if entry.filename in executable_names:
                    mode = 0o755
                path.chmod(mode or 0o644)
    return destination


def write_zip(path, entries):
    """Deterministic ZIP: fixed timestamps, unix creator, explicit modes."""
    if not hasattr(path, "write"):
        path = Path(path)
    directories = {parent.as_posix() + "/" for name, _, _ in entries
                   for parent in PurePosixPath(name).parents if parent.as_posix() != "."}
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, content, mode in sorted([*((name, b"", 0o755) for name in directories), *entries]):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            directory = name.endswith("/")
            info.external_attr = ((stat.S_IFDIR if directory else stat.S_IFREG) | mode) << 16
            info.compress_type = zipfile.ZIP_STORED if directory else zipfile.ZIP_DEFLATED
            archive.writestr(info, content, compresslevel=9)


def config(root):
    """Load + validate 4d/template.json."""
    root = Path(root).resolve()
    config = read_json_file(root / "template.json")
    if config.get("schema") != 1 or config.get("initialized") is not True:
        raise PipelineError("template.json must be initialized with schema 1")
    if not re.fullmatch(r"[a-z][a-z0-9-]{0,62}", config.get("repository", "")):
        raise PipelineError("template.json repository must be the CLI basename form")
    if config.get("namespace") != "WebP" or config.get("feature_class") != "Converter":
        raise PipelineError("template.json identity must stay WebP/Converter in this repository")
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", config.get("version", "")):
        raise PipelineError("component version must be prefix-free SemVer (no v)")
    return config


def load_lock(root):
    """Load + validate the shape of 4d/cli/asset-lock.json."""
    root = Path(root).resolve()
    lock = read_json_file(root / "cli" / "asset-lock.json")
    if lock.get("schema") != 1:
        raise PipelineError("asset-lock schema must be 1")
    if not re.fullmatch(r"[A-Za-z0-9-]+/[A-Za-z0-9-]+", lock.get("repository", "")):
        raise PipelineError("asset-lock repository must be owner/name")
    if not re.fullmatch(r"\d+\.\d+\.\d+", lock.get("version", "")):
        raise PipelineError("asset-lock CLI version must be prefix-free SemVer")
    if lock.get("tag") != "v" + lock["version"]:
        raise PipelineError("asset-lock tag must be the CLI's v-prefixed tag")
    if not re.fullmatch(r"[0-9a-f]{40}", lock.get("source_revision", "")):
        raise PipelineError("asset-lock source_revision must be 40-hex")
    assets = lock.get("assets")
    if not isinstance(assets, dict) or not assets:
        raise PipelineError("asset-lock must pin at least one platform asset")
    for platform, entry in assets.items():
        if platform not in ("macos", "windows"):
            raise PipelineError("unknown asset platform: " + str(platform))
        for field in ("url", "target", "sha256", "manifest_sha256", "binary_sha256"):
            if not isinstance(entry.get(field), str) or not entry[field]:
                raise PipelineError("asset-lock " + platform + "." + field + " is missing")
        if not entry["url"].startswith("https://github.com/" + lock["repository"] + "/releases/download/"):
            raise PipelineError("asset-lock " + platform + " URL must be a repository release asset URL")
        if not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]) \
                or not re.fullmatch(r"[0-9a-f]{64}", entry["manifest_sha256"]) \
                or not re.fullmatch(r"[0-9a-f]{64}", entry["binary_sha256"]):
            raise PipelineError("asset-lock " + platform + " digests must be 64-hex")
    return lock


def repo_root_from(script_root):
    """4d/scripts/... -> the git repository root (two levels up)."""
    return Path(script_root).resolve().parents[2]

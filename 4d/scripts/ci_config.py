#!/usr/bin/env python3
"""Hosted CI configuration: release validation and pinned tool4D download.

Port of the audited quickdraw ci_config subset, re-pointed at the webp
4d/ tree. webp does not need a managed native-build matrix or resource
assembly here: its CLI is built by the Rust workflows and assembled by
``stage_cli.py`` from the verified asset lock.

Commands:
  validate          Fail closed on configuration/manifest identity; with
                    --release also require FOURD_ENABLED=true and a
                    prefix-free SemVer tag equal to the component version.
  download-tool4d   Download a SHA-256-pinned official tool4d app archive
                    into 4d/.tools/tool4d.app (macOS ARM64 hosts only).

The tool4D pin resolves from, in order: the --url/--sha256 pair, the
TOOL4D_URL/TOOL4D_SHA256 environment pair (GitHub Actions vars), or
package.json.tool (only usable once its sha256 is a real 64-hex digest —
"PENDING_*" placeholders force the vars pair).
"""
import argparse
import hashlib
import os
import platform
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import urllib.parse
import urllib.request
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import PipelineError, config, read_json_file

ROOT = Path(__file__).resolve().parents[1]
TOOL4D_DOWNLOAD_HOSTS = {"resources-download.4d.com", "builds2.4d.com"}
SEMVER = r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|[0-9A-Za-z-]+))*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"


def load_config(root=ROOT, *, release=False, repository=None, tag=None):
    """Validated component configuration; release mode adds CI preconditions."""
    cfg = config(root)
    if release:
        if not repository:
            raise PipelineError("GITHUB_REPOSITORY is required for release validation")
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_-]+", repository, re.ASCII):
            raise PipelineError("invalid GitHub repository identity: " + repository)
        if os.environ.get("FOURD_ENABLED") != "true":
            raise PipelineError("releases require FOURD_ENABLED=true")
        if not isinstance(tag, str) or not re.fullmatch(SEMVER, tag, re.ASCII):
            raise PipelineError("release tag must be prefix-free SemVer (for example 20.0.0)")
        if tag != cfg["version"]:
            raise PipelineError("release tag must equal the component version: "
                                + cfg["version"] + " != " + tag)
    return cfg


def validate(root=ROOT, *, release=False, repository=None, tag=None):
    """Full configuration/manifest identity check; returns the loaded config."""
    cfg = load_config(root, release=release, repository=repository, tag=tag)
    manifest = read_json_file(Path(root) / "package.json")
    if manifest.get("name") != cfg["repository"]:
        raise PipelineError("package.json name must equal template.json repository")
    if manifest.get("version") != cfg["version"]:
        raise PipelineError("package.json version must equal template.json version")
    project = Path(root) / "component" / cfg["repository"] / "Project"
    if not (project / (cfg["repository"] + ".4DProject")).is_file():
        raise PipelineError("configured component .4DProject is missing")
    return cfg


# --- tool4D pin + download -------------------------------------------------

def require_tool4d_host():
    if platform.system() != "Darwin" or platform.machine().lower() not in {"arm64", "aarch64"}:
        raise PipelineError("pinned tool4d is macOS ARM64 only; use a matching Apple Silicon host")


def checked_tool_pin(url, digest):
    if not isinstance(url, str) or not isinstance(digest, str):
        raise PipelineError("tool4D URL and SHA256 must be strings")
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != "https" or parsed.hostname not in TOOL4D_DOWNLOAD_HOSTS
            or parsed.username or parsed.password or parsed.port not in (None, 443)
            or parsed.query or parsed.fragment):
        raise PipelineError("tool4D URL must use an official HTTPS host: "
                            + str(parsed.hostname or ""))
    if not re.fullmatch(r"[0-9a-fA-F]{64}", digest):
        raise PipelineError("tool4D SHA256 must be 64 hex digits")
    if any(part.lower() == "latest" for part in
           PurePosixPath(urllib.parse.unquote(parsed.path)).parts):
        raise PipelineError("tool4D URL must pin a concrete build, not latest")
    if not re.search(r"\.(zip|tar\.xz)$", parsed.path, re.IGNORECASE):
        raise PipelineError("tool4D URL must point at a .zip or .tar.xz app archive")
    return url, digest.lower()


def tool_pin(root=ROOT):
    """Derive the official URL from the versioned pin in package.json.tool."""
    tool = read_json_file(Path(root) / "package.json").get("tool")
    if not isinstance(tool, dict):
        raise PipelineError("package.json.tool must specify the approved compiler build and SHA256")
    if (tool.get("platform") != "macos" or tool.get("arch") != "arm"
            or not isinstance(tool.get("branch"), str)
            or not re.fullmatch(r"[0-9]+\.x", tool["branch"])
            or not isinstance(tool.get("version"), str)
            or not re.fullmatch(r"[0-9]+(\.[0-9]+)?[A-Za-z0-9]*", tool["version"])
            or not isinstance(tool.get("build"), str) or not re.fullmatch(r"[1-9][0-9]*", tool["build"])
            or not isinstance(tool.get("sha256"), str)):
        raise PipelineError("package.json.tool requires fixed branch/version/build, macos/arm and a SHA256")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", tool["sha256"]):
        raise PipelineError("package.json.tool.sha256 is not a real 64-hex digest; "
                            "set the TOOL4D_URL/TOOL4D_SHA256 vars pair instead, or "
                            "record the verified build tool sha256 in package.json")
    url = (f"https://resources-download.4d.com/release/{tool['branch']}/"
           f"{tool['version']}/{tool['build']}/mac/tool4d_arm64.tar.xz")
    return checked_tool_pin(url, tool["sha256"])


def resolve_tool_pin(root=ROOT, *, url=None, digest=None):
    if url is not None or digest is not None:
        if not url or not digest:
            raise PipelineError("--url and --sha256 must be supplied together and nonempty")
        return checked_tool_pin(url, digest)
    env_url, env_digest = os.environ.get("TOOL4D_URL", ""), os.environ.get("TOOL4D_SHA256", "")
    if env_url or env_digest:
        if not env_url or not env_digest:
            raise PipelineError("TOOL4D_URL and TOOL4D_SHA256 overrides must be supplied together")
        return checked_tool_pin(env_url, env_digest)
    return tool_pin(root)


def checked_download_url(url):
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != "https" or parsed.hostname not in TOOL4D_DOWNLOAD_HOSTS
            or parsed.username or parsed.password or parsed.port not in (None, 443)):
        raise PipelineError("tool4D download redirected outside the official HTTPS origin")
    return url


class Tool4DRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, newurl):
        checked_download_url(newurl)
        return super().redirect_request(request, response, code, message, headers, newurl)


def open_tool4d_url(url):
    checked_download_url(url)
    return urllib.request.build_opener(Tool4DRedirectHandler()).open(url, timeout=120)


def _checked_archive_paths(entries, tools):
    """Safety gates shared by both archive formats; returns symlink targets."""
    base = tools.resolve()
    names, destinations, links = set(), set(), {}
    for name, kind, linkname in entries:
        path = PurePosixPath(name)
        if (not name or path.is_absolute() or ".." in path.parts or "\\" in name
                or ":" in name or kind not in ("dir", "file", "symlink")):
            raise PipelineError("unsafe path or unsupported entry in tool4D archive: " + name)
        if name in names:
            raise PipelineError("duplicate tool4D archive path: " + name)
        names.add(name)
        destination = tools.joinpath(*path.parts)
        if (destination in destinations
                or not destination.resolve().is_relative_to(base)
                or destination.is_symlink()
                or any(parent.is_symlink() for parent in destination.parents)):
            raise PipelineError("unsafe tool4D extraction destination: " + name)
        destinations.add(destination)
        if kind == "symlink":
            target = PurePosixPath(linkname)
            if (not linkname or target.is_absolute() or "\\" in linkname or ":" in linkname
                    or not (destination.parent / linkname).resolve().is_relative_to(base)):
                raise PipelineError("unsafe tool4D symlink target: " + linkname)
            links[destination] = linkname
    if any(parent in links for destination in destinations for parent in destination.parents):
        raise PipelineError("tool4D archive entries must not traverse a symlink")
    return links


def _create_links(links, base):
    created = []
    try:
        for destination, target in links.items():
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.symlink_to(target)
            created.append(destination)
        for destination in created:
            if not destination.resolve().is_relative_to(base):
                raise PipelineError("tool4D symlink chain escapes the extraction directory")
    except (ValueError, OSError, RuntimeError, PipelineError):
        for destination in created:
            destination.unlink(missing_ok=True)
        raise


def _extract_tar(archive, tools):
    base = tools.resolve()
    with tarfile.open(archive, "r:xz") as packed:
        entries = [(member.name,
                    "dir" if member.isdir() else "symlink" if member.issym()
                    else "file" if member.isfile() else "other",
                    member.linkname)
                   for member in packed.getmembers()]
        links = _checked_archive_paths(entries, tools)
        for member in packed.getmembers():
            destination = tools.joinpath(*PurePosixPath(member.name).parts)
            if member.isdir():
                destination.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                destination.parent.mkdir(parents=True, exist_ok=True)
                with packed.extractfile(member) as source, destination.open("wb") as output:
                    shutil.copyfileobj(source, output)
                mode = member.mode & 0o777
                destination.chmod((mode & 0o111) and mode or 0o644)
        _create_links(links, base)


def _extract_zip(archive, tools):
    base = tools.resolve()
    with zipfile.ZipFile(archive) as packed:
        entries = []
        for info in packed.infolist():
            mode = (info.external_attr >> 16) & 0xFFFF
            kind = ("dir" if info.is_dir()
                    else "symlink" if stat.S_IFMT(mode) == stat.S_IFLNK else "file")
            target = ""
            if kind == "symlink":
                target = packed.read(info).decode("utf-8", "strict")
            entries.append((info.filename, kind, target))
        links = _checked_archive_paths(entries, tools)
        for info, (_, kind, target) in zip(packed.infolist(), entries):
            destination = tools.joinpath(*PurePosixPath(info.filename).parts)
            if kind == "dir":
                destination.mkdir(parents=True, exist_ok=True)
            else:
                destination.parent.mkdir(parents=True, exist_ok=True)
                with packed.open(info) as source, destination.open("wb") as output:
                    shutil.copyfileobj(source, output)
                mode = ((info.external_attr >> 16) & 0o777) or 0o644
                destination.chmod(((mode & 0o111) and mode) or 0o644)
        _create_links(links, base)


def download_tool4d(root=ROOT, *, url=None, digest=None):
    """Download a concrete, SHA-256-pinned HTTPS TAR.XZ or ZIP app archive."""
    root = Path(root).resolve()
    require_tool4d_host()
    url, digest = resolve_tool_pin(root, url=url, digest=digest)
    tools = root / ".tools"
    if any(path.is_symlink() for path in [tools, *tools.parents]):
        raise PipelineError(".tools must not be a symlink")
    tools.mkdir(exist_ok=True)
    archive = tools / "tool4d.download"
    if archive.is_symlink():
        raise PipelineError("tool4D download destination must not be a symlink")
    sha = hashlib.sha256()
    try:
        with open_tool4d_url(url) as response, archive.open("wb") as output:
            checked_download_url(response.geturl())
            while chunk := response.read(1024 * 1024):
                sha.update(chunk)
                output.write(chunk)
    except (OSError, PipelineError):
        archive.unlink(missing_ok=True)
        raise
    if sha.hexdigest().lower() != digest.lower():
        archive.unlink(missing_ok=True)
        raise PipelineError("tool4D download does not match the pinned SHA256")
    destination = tools / "tool4d.app"
    staging = tools / "tool4d.unpacking"
    if destination.is_symlink() or staging.is_symlink():
        raise PipelineError("tool4D extraction destination must not be a symlink")
    shutil.rmtree(destination, ignore_errors=True)
    shutil.rmtree(staging, ignore_errors=True)
    try:
        staging.mkdir()
        if url.lower().endswith(".zip"):
            _extract_zip(archive, staging)
        else:
            _extract_tar(archive, staging)
        candidates = [path for path in staging.iterdir() if path.name.startswith("tool4d")]
        if (len(candidates) != 1 or not candidates[0].is_dir()
                or not (candidates[0] / "Contents" / "MacOS" / "tool4d").is_file()):
            raise PipelineError("tool4D archive must contain exactly one tool4d app "
                                "with a Contents/MacOS/tool4d binary")
        candidates[0].rename(destination)
    except (ValueError, OSError, KeyError, TypeError, tarfile.TarError, zipfile.BadZipFile):
        shutil.rmtree(destination, ignore_errors=True)
        raise
    finally:
        shutil.rmtree(staging, ignore_errors=True)
        archive.unlink(missing_ok=True)
    binary = destination / "Contents" / "MacOS" / "tool4d"
    binary.chmod(0o755)
    return str(binary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("validate", "download-tool4d"))
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--tag", help="prefix-free SemVer release tag; defaults to GITHUB_REF_NAME")
    parser.add_argument("--url", help="optional official URL override, paired with --sha256")
    parser.add_argument("--sha256", help="optional approved hash override; otherwise env pair/package.json.tool")
    args = parser.parse_args()
    try:
        if args.command == "download-tool4d":
            print(download_tool4d(args.root, url=args.url, digest=args.sha256))
            return
        tag = (args.tag if args.tag is not None else os.environ.get("GITHUB_REF_NAME", "")) \
            if args.release else None
        validate(args.root, release=args.release,
                 repository=os.environ.get("GITHUB_REPOSITORY") if args.release else None,
                 tag=tag)
        print("configuration validated")
    except (PipelineError, ValueError, OSError, KeyError, TypeError, RuntimeError,
            tarfile.TarError, zipfile.BadZipFile, subprocess.SubprocessError) as error:
        parser.exit(2, f"delivery configuration failed: {error}\n")


if __name__ == "__main__":
    sys.exit(main())

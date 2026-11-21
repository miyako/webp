#!/usr/bin/env python3
"""Verify accepted release assets before creating the tag and publishing.

Port of the audited quickdraw release_assets, re-pointed at the webp 4d/
tree and the two-track policy: the component release tag is prefix-free
SemVer and its asset is dist/<component>.zip (+ evidence; + DMG and
signing proof when Apple-signed). The CLI's v-tagged release assets are
not published here (see ADR-012).

verify (default) checks, fail-closed, that every release asset on disk
matches the accepted hosted evidence byte for byte. publish additionally
creates the tag on the verified commit (main-ancestry, collision-checked)
and runs `gh release create` with the verified assets.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import PipelineError, config, regular, repo_root_from, sha256
from release_ref import create_tag as create_release_tag, verify as verify_tag
from sign_release import release_policy, submission_uuid

ROOT = Path(__file__).resolve().parents[1]


def commit(root):
    """HEAD of the git repository containing this checkout, else None.

    The scripts live under <repo>/4d/scripts, so the toplevel is the repo
    root, not the 4d root; verify against that.
    """
    result = subprocess.run(["git", "rev-parse", "--show-toplevel", "HEAD"], cwd=root,
                            capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        return None
    lines = result.stdout.splitlines()
    if len(lines) != 2 or Path(lines[0]).resolve() != repo_root_from(Path(__file__)):
        return None
    return lines[1]


def acceptance_assets(root, cfg):
    """The dist/ evidence: report, final archive, and signing artifacts if any."""
    dist = Path(root) / "dist"
    prefix = cfg["repository"]
    report = json.loads(regular(dist / (prefix + ".acceptance.json")).read_text(encoding="utf-8"))
    if (not isinstance(report, dict) or report.get("schema") != 1
            or report.get("scope") != "hosted" or report.get("finished") is not True
            or report.get("result") != "partial" or not isinstance(report.get("artifact"), dict)):
        raise PipelineError("hosted acceptance evidence is not a finished partial-pass report")
    artifact = report["artifact"]
    config_version = cfg["version"]
    # quickdraw parity: the evidence identifies the archive by root-relative
    # path so a different runner OS (publish job) can still verify it.
    expected_path = "dist/" + prefix + ".zip"
    if (artifact.get("path") != expected_path
            or not re.fullmatch(r"[0-9a-f]{64}", str(artifact.get("sha256")))
            or not isinstance(artifact.get("bytes"), int)):
        raise PipelineError("acceptance evidence does not identify the final archive (expected path " + expected_path + ")")
    assets = [{
        "path": "dist/" + prefix + ".zip", "sha256": artifact["sha256"],
        "bytes": artifact["bytes"], "signed": artifact.get("signed") is True,
    }]
    assets.append({
        "path": "dist/" + prefix + ".acceptance.json",
        "sha256": sha256(regular(dist / (prefix + ".acceptance.json"))),
        "bytes": (dist / (prefix + ".acceptance.json")).stat().st_size,
        "signed": False,
    })
    signing = dist / (prefix + "-signing.json")
    if artifact.get("signed") is True:
        assets.append({
            "path": "dist/" + prefix + "-signing.json",
            "sha256": sha256(regular(signing)),
            "bytes": signing.stat().st_size, "signed": False,
        })
        dmg = dist / (prefix + ".dmg")
        proof = json.loads(regular(signing).read_text(encoding="utf-8"))
        if (proof.get("releaseTag") != config_version
                or not isinstance(proof.get("zip"), dict)
                or proof["zip"].get("sha256") != assets[0]["sha256"]
                or proof["zip"].get("stapled") is not False
                or proof["zip"].get("notarizedViaDMG") is not True
                or proof["zip"].get("origin") != "final-mounted-accepted-DMG"
                or not isinstance(proof.get("dmg"), dict)
                or proof["dmg"].get("stapled") is not True
                or proof["dmg"].get("validated") is not True
                or not isinstance(proof["dmg"].get("sha256"), str)):
            raise PipelineError("signing proof is missing a stapled, validated DMG record")
        proof_dmg = proof["dmg"]
        assets.append({
            "path": "dist/" + prefix + ".dmg", "sha256": proof_dmg["sha256"],
            "bytes": dmg.stat().st_size, "signed": True,
        })
    elif signing.exists() or (dist / (prefix + ".dmg")).exists():
        raise PipelineError("signing artifacts present for an unsigned acceptance")
    for asset in assets:
        path = dist / Path(asset["path"]).name
        if path.is_symlink() or any(item.is_symlink() for item in path.parents):
            raise PipelineError("release asset path must not traverse symlinks: " + asset["path"])
    return report, assets


def verify_assets(root, repository, tag, revision, apple_signing=False):
    root = Path(root).resolve()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_-]+", repository, re.ASCII):
        raise PipelineError("invalid GitHub repository identity")
    cfg = config(root)
    if tag != cfg["version"]:
        raise PipelineError("release tag must equal the component version: "
                            + cfg["version"] + " != " + tag)
    if commit(root) != revision:
        raise PipelineError("checked-out HEAD does not match the release commit")
    if release_policy(root) and not apple_signing:
        raise PipelineError("publication cannot downgrade the version signing policy")
    if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise PipelineError("asset verification requires the normalized release SHA")
    report, assets = acceptance_assets(root, cfg)
    for asset in assets:
        path = root / asset["path"]
        if (not re.fullmatch(r"[0-9a-f]{64}", asset["sha256"])
                or not path.is_file()
                or path.stat().st_size != asset["bytes"]
                or sha256(path) != asset["sha256"]):
            raise PipelineError("release asset does not match accepted bytes: " + asset["path"])
    if assets[0].get("signed"):
        proof = json.loads(regular(root / "dist" / (cfg["repository"] + "-signing.json"))
                           .read_text(encoding="utf-8"))
        if (proof.get("sourceSHA") != revision or proof.get("releaseTag") != cfg["version"]
                or proof.get("unsignedZIPHashSHA256") != report.get("unsigned_artifact", {}).get("sha256")):
            raise PipelineError("signing proof does not match the accepted release provenance")
        submission_uuid(proof.get("notarization", {}).get("submissionUUID"))
    filenames = {Path(asset["path"]).name for asset in assets}
    if {path.name for path in (root / "dist").iterdir()} != filenames:
        raise PipelineError("release artifact directory contains unvalidated assets")
    return assets


def publish(root, repository, tag, revision, apple_signing=False, *, create_tag=False):
    assets = verify_assets(root, repository, tag, revision, apple_signing)
    if create_tag:
        create_release_tag(root, repository, tag, revision)
    else:
        verify_tag(root, tag, revision)
    cfg = config(root)
    signed = assets[0].get("signed")
    notes = (
        "Compiled 4D component with the pinned webp-convert CLI preinstalled. "
        "Class/method source is excluded; the exact final ZIP passed hosted compiled-host "
        "acceptance on a macOS ARM64 runner with the pinned tool4d. "
        + ("The macOS CLI and native libraries are Developer ID signed; the DMG is notarized "
           "and stapled; the ZIP was repacked from that final mounted DMG. "
           if signed else "Unsigned release; no Apple credentials were used. ")
        + "Adjacent acceptance evidence binds the assets to source revision " + revision + ". "
        "Native Intel 4D and Windows 4D checks remain separate observations.")
    paths = [root / asset["path"] for asset in assets]
    subprocess.run(["gh", "release", "create", tag, *(str(path) for path in paths),
                    "--repo", repository, "--verify-tag", "--title", tag,
                    "--notes", notes], cwd=root, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--require-apple-signing", action="store_true")
    parser.add_argument("--publish", action="store_true",
                        help="publish verified assets; default only verifies without creating tags/releases")
    parser.add_argument("--create-tag", action="store_true",
                        help="create a new main-only version tag after asset verification; requires --publish")
    args = parser.parse_args()
    try:
        if args.create_tag and not args.publish:
            raise ValueError("--create-tag requires --publish")
        if args.publish:
            publish(args.root, args.repository, args.tag, args.sha,
                    args.require_apple_signing, create_tag=args.create_tag)
        else:
            for asset in verify_assets(args.root, args.repository, args.tag, args.sha,
                                       args.require_apple_signing):
                print(asset)
    except (ValueError, OSError, KeyError, TypeError, PipelineError,
            subprocess.SubprocessError) as error:
        parser.exit(2, f"release asset verification/publication failed: {error}\n")


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Fail closed unless a stable CLI release has six verified native builds."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys


REPOSITORY = "IPA-CyberLab/IPA-RS-HeteroCloud"
IMAGE = "ghcr.io/ipa-cyberlab/ipa-rs-heterocloud@sha256:"
TAG = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z", re.ASCII)
SHA = re.compile(r"[a-f0-9]{40}\Z", re.ASCII)
DIGEST = re.compile(r"[a-f0-9]{64}\Z", re.ASCII)
TARGETS = (("linux", "x64", "tar.gz"), ("linux", "arm64", "tar.gz"),
           ("macos", "x64", "tar.gz"), ("macos", "arm64", "tar.gz"),
           ("windows", "x64", "zip"), ("windows", "arm64", "zip"))


def version(tag):
    match = TAG.fullmatch(tag)
    if not match:
        raise ValueError("promotion requires a canonical stable version tag")
    return tuple(int(value) for value in match.groups())


def required_assets(tag):
    names = [f"heterocloud-{tag}-{os_name}-{arch}.{extension}" for os_name, arch, extension in TARGETS]
    return names + [name + ".sha256" for name in names] + ["heterocloud-release-artifact.json"]


def validate_metadata(tag, release, run, commit, latest, artifact):
    target_version = version(tag)
    if not SHA.fullmatch(commit):
        raise ValueError("invalid tagged commit")
    if (release.get("tag_name") != tag or release.get("draft") is not False
            or not isinstance(release.get("id"), int) or release["id"] <= 0):
        raise ValueError("release must be a published release for this tag")
    if latest and version(latest["tag_name"]) > target_version:
        raise ValueError("refusing to replace a newer stable release")
    if (run.get("repository", {}).get("full_name") != REPOSITORY
            or run.get("path") != ".github/workflows/release.yml"
            or run.get("event") != "release" or run.get("head_branch") != tag
            or run.get("head_sha") != commit or run.get("status") != "completed"
            or run.get("conclusion") != "success"):
        raise ValueError("matching release workflow has not completed successfully")
    image = artifact.get("image", "")
    if (artifact.get("schema_version") != 1 or artifact.get("component") != "heterocloud"
            or artifact.get("version") != tag[1:] or artifact.get("commit") != commit
            or not image.startswith(IMAGE) or not DIGEST.fullmatch(image.removeprefix(IMAGE))):
        raise ValueError("release artifact does not match the exact tag and commit")
    assets = release.get("assets", [])
    names = [asset.get("name") for asset in assets]
    if len(names) != len(set(names)):
        raise ValueError("release contains duplicated asset names")
    by_name = {asset["name"]: asset for asset in assets}
    for name in required_assets(tag):
        asset = by_name.get(name)
        if not asset or asset.get("state") != "uploaded" or asset.get("size", 0) <= 0:
            raise ValueError("required release asset is missing or not uploaded: " + name)
    return image


def validate_checksums(tag, directory):
    for name in required_assets(tag)[:len(TARGETS)]:
        archive = directory / name
        checksum_path = directory / (name + ".sha256")
        if archive.is_symlink() or checksum_path.is_symlink() or not archive.is_file() or not checksum_path.is_file():
            raise ValueError("downloaded assets must be regular files")
        fields = checksum_path.read_text(encoding="utf-8").strip().split()
        if len(fields) != 2 or not DIGEST.fullmatch(fields[0]) or fields[1].lstrip("*") != name:
            raise ValueError("invalid checksum file for " + name)
        digest = hashlib.sha256()
        with archive.open("rb") as file:
            for chunk in iter(lambda: file.read(1024 * 1024), b""):
                digest.update(chunk)
        if digest.hexdigest() != fields[0]:
            raise ValueError("checksum mismatch for " + name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--release", type=Path, required=True)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--tag-commit", type=Path, required=True)
    parser.add_argument("--latest", type=Path, required=True)
    parser.add_argument("--assets", type=Path, required=True)
    args = parser.parse_args()
    read = lambda p: json.loads(p.read_text(encoding="utf-8"))
    release, run, commit, latest = map(read, (args.release, args.run, args.tag_commit, args.latest))
    artifact = read(args.assets / "heterocloud-release-artifact.json")
    image = validate_metadata(args.tag, release, run, commit["sha"], latest, artifact)
    validate_checksums(args.tag, args.assets)
    print(json.dumps({"release_id": release["id"], "tag": args.tag, "image": image, "commit": commit["sha"]}))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, json.JSONDecodeError) as error:
        sys.exit("Stable promotion rejected: " + str(error))

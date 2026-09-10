#!/usr/bin/env python3
"""Release identity and immutable channel artifacts; never builds or publishes."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

COMPONENT = "heterocloud"
IMAGE = "ghcr.io/ipa-cyberlab/ipa-rs-heterocloud"
TAG = re.compile(r"v?\d+\.\d+\.\d+(?:[.-][A-Za-z0-9._-]+)?", re.ASCII)
SHA = re.compile(r"[a-f0-9]{40}")
DIGEST = re.compile(r"sha256:[a-f0-9]{64}")


def validate_release(tag, commit, head, tag_commit):
    if not isinstance(tag, str) or len(tag) > 128 or not TAG.fullmatch(tag):
        raise ValueError("release tag is incompatible with channel/image grammar")
    if not all(isinstance(v, str) and SHA.fullmatch(v) for v in (commit, head, tag_commit)):
        raise ValueError("release commits must be full lowercase 40-character SHAs")
    if commit != head or commit != tag_commit:
        raise ValueError("release event, HEAD and exact tag commit differ")
    return tag.removeprefix("v")


def release():
    tag, commit = os.environ["RELEASE_TAG"], os.environ["GITHUB_SHA"]
    validate_release(tag, commit, commit, commit)
    def rev(ref):
        return subprocess.check_output(
            ["git", "rev-parse", "--verify", ref], text=True
        ).strip()
    version = validate_release(tag, commit, rev("HEAD^{commit}"), rev(f"refs/tags/{tag}^{{commit}}"))
    return version, commit


def allows_latest(version, prerelease):
    return prerelease == "false" and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) is not None


def artifact(version, commit, digest):
    if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
        raise ValueError("missing or invalid pushed image digest")
    validate_release(version, commit, commit, commit)
    return dict(schema_version=1, component=COMPONENT, version=version,
                commit=commit, image=f"{IMAGE}@{digest}")


def sources(version, commit, records):
    if len(records) != 2:
        raise ValueError("both architecture results are required")
    refs = []
    for record in records:
        ref = record.get("image", "")
        digest = ref.removeprefix(IMAGE + "@")
        if record != artifact(version, commit, digest):
            raise ValueError("architecture result does not match this release")
        refs.append(ref)
    if len(set(refs)) != 2:
        raise ValueError("architecture results must be distinct")
    return refs


def final_digest(metadata):
    descriptor = metadata.get("containerimage.descriptor", {})
    digest = descriptor.get("digest")
    if descriptor.get("mediaType") not in (
        "application/vnd.oci.image.index.v1+json",
        "application/vnd.docker.distribution.manifest.list.v2+json",
    ) or not isinstance(digest, str) or not DIGEST.fullmatch(digest):
        raise ValueError("missing final multi-architecture index descriptor")
    return digest


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("validate", "generate", "sources"))
    parser.add_argument("--metadata")
    parser.add_argument("files", nargs="*")
    args = parser.parse_args()
    version, commit = release()
    if args.command == "validate":
        cargo = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]["package"]["version"]
        # This is source metadata, not an assertion that Cargo equals the release tag.
        if not isinstance(cargo, str) or not cargo or len(cargo) > 128 or "\n" in cargo or "\r" in cargo:
            raise ValueError("invalid source Cargo version metadata")
        print(f"version={version}\ncargo_version={cargo}")
        print(f"latest={str(allows_latest(version, os.environ.get('RELEASE_PRERELEASE'))).lower()}")
    elif args.command == "generate":
        digest = (final_digest(json.loads(Path(args.metadata).read_text()))
                  if args.metadata else os.environ["IMAGE_DIGEST"])
        print(json.dumps(artifact(version, commit, digest), indent=2))
    else:
        print("\n".join(sources(version, commit, [json.loads(Path(p).read_text()) for p in args.files])))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Release artifact rejected: {error}") from None

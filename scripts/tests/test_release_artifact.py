"""Offline fixtures only; synthetic hashes never become publishing outputs."""
import copy
import importlib.util
import io
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("release_artifact", Path(__file__).parents[1] / "release_artifact.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    commit = "a" * 40
    digest = "sha256:" + "b" * 64

    def test_source_cargo_version_is_not_release_version(self):
        output = io.StringIO()
        with patch.object(release, "release", return_value=("1.2.3-dev.1", self.commit)), \
             patch("sys.argv", ["release_artifact.py", "validate"]), \
             patch.object(release.Path, "read_text", return_value='[workspace.package]\nversion = "0.9.0+source"'), \
             patch("sys.stdout", output):
            release.main()
        self.assertIn("version=1.2.3-dev.1\n", output.getvalue())
        self.assertIn("cargo_version=0.9.0+source\n", output.getvalue())

    def test_latest_requires_stable_tag_and_explicit_non_prerelease(self):
        self.assertTrue(release.allows_latest("1.2.3", "false"))
        for version, flag in (("1.2.3-dev.1", "false"), ("1.2.3", "true"), ("1.2.3", None)):
            self.assertFalse(release.allows_latest(version, flag))

    def test_release_resolves_exact_qualified_tag(self):
        with patch.dict(release.os.environ, {"RELEASE_TAG": "v1.2.3-dev.1", "GITHUB_SHA": self.commit}), \
             patch.object(release.subprocess, "check_output", return_value=self.commit + "\n") as git:
            self.assertEqual(release.release(), ("1.2.3-dev.1", self.commit))
            self.assertEqual([call.args[0][-1] for call in git.call_args_list],
                             ["HEAD^{commit}", "refs/tags/v1.2.3-dev.1^{commit}"])

    def test_bad_tag_rejected_before_git(self):
        with patch.dict(release.os.environ, {"RELEASE_TAG": "v1.2.3+build", "GITHUB_SHA": self.commit}), \
             patch.object(release.subprocess, "check_output") as git:
            with self.assertRaises(ValueError):
                release.release()
            git.assert_not_called()

    def test_stable_and_dev_tags(self):
        for tag in ("v1.2.3", "1.2.3", "v1.2.3-dev.4"):
            self.assertEqual(release.validate_release(tag, *([self.commit] * 3)), tag.removeprefix("v"))

    def test_incompatible_tags(self):
        for tag in ("v1.2.3+build.1", "master", "-bad", "v1.2.3\n", "v1.2.3-" + "a" * 128):
            with self.assertRaises(ValueError):
                release.validate_release(tag, *([self.commit] * 3))

    def test_commit_and_exact_tag_mismatch(self):
        for triple in ((self.commit, "b" * 40, self.commit),
                       (self.commit, self.commit, "b" * 40),
                       ("a" * 7,) * 3, ("A" * 40,) * 3):
            with self.assertRaises(ValueError):
                release.validate_release("v1.2.3", *triple)

    def test_artifact_exact_contract(self):
        result = release.artifact("1.2.3-dev.1", self.commit, self.digest)
        self.assertEqual(set(result), {"schema_version", "component", "version", "commit", "image"})
        self.assertEqual(result["image"], release.IMAGE + "@" + self.digest)

    def test_digest_required(self):
        for digest in ("latest", "sha256:" + "A" * 64, "", None, self.digest + "\n"):
            with self.assertRaises(ValueError):
                release.artifact("1.2.3", self.commit, digest)

    def test_immutable_architecture_sources(self):
        records = [release.artifact("1.2.3", self.commit, "sha256:" + c * 64) for c in ("b", "c")]
        self.assertEqual(release.sources("1.2.3", self.commit, records), [r["image"] for r in records])
        for field, value in (("commit", "d" * 40), ("version", "1.2.4"), ("component", "flash"),
                             ("image", release.IMAGE + ":1.2.3-amd64")):
            bad = copy.deepcopy(records)
            bad[0][field] = value
            with self.assertRaises(ValueError):
                release.sources("1.2.3", self.commit, bad)
        for bad in (records[:1], [records[0], records[0]]):
            with self.assertRaises(ValueError):
                release.sources("1.2.3", self.commit, bad)

    def test_final_index_not_architecture_metadata(self):
        descriptor = {"mediaType": "application/vnd.oci.image.index.v1+json", "digest": self.digest}
        self.assertEqual(release.final_digest({"containerimage.descriptor": descriptor}), self.digest)
        for metadata in ({}, {"containerimage.digest": self.digest},
                         {"containerimage.descriptor": {**descriptor, "mediaType": "application/vnd.oci.image.manifest.v1+json"}}):
            with self.assertRaises(ValueError):
                release.final_digest(metadata)

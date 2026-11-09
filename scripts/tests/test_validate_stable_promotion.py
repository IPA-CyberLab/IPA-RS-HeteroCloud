import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("promotion", Path(__file__).resolve().parents[1] / "validate_stable_promotion.py")
promotion = importlib.util.module_from_spec(spec)
spec.loader.exec_module(promotion)
TAG = "v0.1.116"
COMMIT = "a" * 40
IMAGE = promotion.IMAGE + "b" * 64
RELEASE = {"id": 123, "tag_name": TAG, "draft": False, "prerelease": True,
           "assets": [{"name": name, "state": "uploaded", "size": 1} for name in promotion.required_assets(TAG)]}
RUN = {"repository": {"full_name": promotion.REPOSITORY}, "path": ".github/workflows/release.yml",
       "event": "release", "head_branch": TAG, "head_sha": COMMIT, "status": "completed", "conclusion": "success"}
ARTIFACT = {"schema_version": 1, "component": "heterocloud", "version": "0.1.116", "commit": COMMIT, "image": IMAGE}


class StablePromotionTests(unittest.TestCase):
    def test_valid_release_and_idempotent_promotion(self):
        self.assertEqual(promotion.validate_metadata(TAG, RELEASE, RUN, COMMIT, {"tag_name": "v0.1.115"}, ARTIFACT), IMAGE)
        self.assertEqual(promotion.validate_metadata(TAG, {**RELEASE, "prerelease": False}, RUN, COMMIT, {"tag_name": TAG}, ARTIFACT), IMAGE)

    def test_no_newer_stable_or_incomplete_release(self):
        with self.assertRaisesRegex(ValueError, "newer"):
            promotion.validate_metadata(TAG, RELEASE, RUN, COMMIT, {"tag_name": "v0.1.117"}, ARTIFACT)
        with self.assertRaisesRegex(ValueError, "missing"):
            promotion.validate_metadata(TAG, {**RELEASE, "assets": RELEASE["assets"][:-1]}, RUN, COMMIT, None, ARTIFACT)
        with self.assertRaisesRegex(ValueError, "matching release workflow"):
            promotion.validate_metadata(TAG, RELEASE, {**RUN, "conclusion": "failure"}, COMMIT, None, ARTIFACT)
        with self.assertRaisesRegex(ValueError, "matching release workflow"):
            promotion.validate_metadata(TAG, RELEASE, {**RUN, "head_sha": "c" * 40}, COMMIT, None, ARTIFACT)
        with self.assertRaisesRegex(ValueError, "exact tag"):
            promotion.validate_metadata(TAG, RELEASE, RUN, COMMIT, None, {**ARTIFACT, "commit": "c" * 40})

    def test_checksum_verifies_all_archives_and_rejects_tampering(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for index, name in enumerate(promotion.required_assets(TAG)[:len(promotion.TARGETS)]):
                contents = f"fixture-{index}".encode()
                (root / name).write_bytes(contents)
                (root / (name + ".sha256")).write_text(f"{hashlib.sha256(contents).hexdigest()}  {name}\n")
            promotion.validate_checksums(TAG, root)
            (root / promotion.required_assets(TAG)[0]).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                promotion.validate_checksums(TAG, root)

    def test_rejects_noncanonical_tag(self):
        for tag in ("v0.1.116-beta", "0.1.116", "v00.1.116", "v0.1.116/../../x"):
            with self.assertRaises(ValueError):
                promotion.version(tag)


if __name__ == "__main__":
    unittest.main()

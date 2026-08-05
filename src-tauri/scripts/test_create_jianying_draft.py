import json
import unittest
from pathlib import Path
from subprocess import CompletedProcess
from tempfile import TemporaryDirectory
from unittest.mock import patch

from create_jianying_draft import (
    fit_source_duration,
    jianying_is_running,
    register_draft,
    register_when_safe,
)


class SourceDurationBoundaryTests(unittest.TestCase):
    def test_keeps_a_source_range_that_fits(self):
        self.assertEqual(fit_source_duration(1_000_000, 2_000_000, 3_000_000), 2_000_000)

    def test_clamps_a_small_parser_rounding_difference(self):
        self.assertEqual(fit_source_duration(6_670_000, 2_641_000, 9_300_000), 2_630_000)

    def test_rejects_a_material_source_overrun(self):
        with self.assertRaises(RuntimeError):
            fit_source_duration(6_670_000, 2_681_000, 9_300_000)


class JianyingProcessDetectionTests(unittest.TestCase):
    @patch("create_jianying_draft.subprocess.run")
    def test_detects_jianying_without_decoding_tasklist_output(self, run):
        run.return_value = CompletedProcess(
            args=[],
            returncode=0,
            stdout=b'"JianyingPro.exe","1234","Console","1","100 K"',
            stderr=b"",
        )

        self.assertTrue(jianying_is_running())

    @patch("create_jianying_draft.subprocess.run")
    def test_treats_missing_tasklist_output_as_not_running(self, run):
        run.return_value = CompletedProcess(
            args=[],
            returncode=0,
            stdout=None,
            stderr=None,
        )

        self.assertFalse(jianying_is_running())


class DeferredRegistrationTests(unittest.TestCase):
    @patch("create_jianying_draft.jianying_is_running", return_value=True)
    @patch("create_jianying_draft.register_draft")
    def test_defers_registration_while_jianying_is_running(self, register, _running):
        status = register_when_safe("registry", "root", "draft", "name", 1_000)

        self.assertEqual(status, "pending")
        register.assert_not_called()

    @patch("create_jianying_draft.jianying_is_running", return_value=False)
    @patch("create_jianying_draft.register_draft")
    def test_registers_immediately_when_jianying_is_closed(self, register, _running):
        status = register_when_safe("registry", "root", "draft", "name", 1_000)

        self.assertEqual(status, "registered")
        register.assert_called_once_with("registry", "root", "draft", "name", 1_000)

    def test_registration_is_idempotent_for_an_existing_draft(self):
        with TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            draft_path = root / "pending-draft"
            draft_path.mkdir()
            (draft_path / "draft_content.json").write_text("{}", encoding="utf-8")
            registry_path = root / "root_meta_info.json"
            registry_path.write_text(
                json.dumps(
                    {
                        "all_draft_store": [
                            {
                                "draft_fold_path": str(draft_path),
                                "draft_root_path": str(root),
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )

            register_draft(
                registry_path,
                root,
                draft_path,
                "pending-draft",
                1_000,
            )

            registry = json.loads(registry_path.read_text(encoding="utf-8"))
            self.assertEqual(len(registry["all_draft_store"]), 1)


if __name__ == "__main__":
    unittest.main()

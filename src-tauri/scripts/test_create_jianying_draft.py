"""验证 Jianying 适配器的目录安全、版本交接、注册与失败回滚行为。"""

import json
import sys
import unittest
from pathlib import Path
from subprocess import CompletedProcess
from tempfile import TemporaryDirectory
from unittest.mock import Mock, patch

from pyJianYingDraft import TrackType

from create_jianying_draft import (
    add_music_tracks,
    add_text_tracks,
    escape_text_material_unicode,
    fit_source_duration,
    jianying_is_running,
    main,
    new_draft_path,
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


class TextTrackExportTests(unittest.TestCase):
    @patch("create_jianying_draft.add_supported_text_animation")
    @patch("create_jianying_draft.jianying_text_shadow", return_value=None)
    @patch("create_jianying_draft.jianying_text_background", return_value=None)
    @patch("create_jianying_draft.jianying_text_border", return_value=None)
    @patch("create_jianying_draft.jianying_font", return_value=None)
    @patch("create_jianying_draft.TextSegment")
    def test_keeps_overlapping_text_on_independent_layered_tracks(
        self,
        text_segment,
        _font,
        _border,
        _background,
        _shadow,
        _animation,
    ):
        script = Mock()
        script.materials.texts = []
        text_segment.side_effect = [Mock(name="lower"), Mock(name="upper")]
        tracks = [
            {
                "id": "upper",
                "layer": 4,
                "enabled": True,
                "cues": [{"id": "upper-cue", "startMs": 0, "endMs": 1_000, "text": "upper"}],
            },
            {
                "id": "lower",
                "layer": 1,
                "enabled": True,
                "cues": [{"id": "lower-cue", "startMs": 0, "endMs": 1_000, "text": "lower"}],
            },
        ]

        add_text_tracks(script, tracks)

        self.assertEqual(
            [call.kwargs for call in script.add_track.call_args_list],
            [
                {"track_name": "assembly-text-layer-1-1", "relative_index": 1},
                {"track_name": "assembly-text-layer-4-0", "relative_index": 4},
            ],
        )
        self.assertEqual(
            [call.kwargs["track_name"] for call in script.add_segment.call_args_list],
            ["assembly-text-layer-1-1", "assembly-text-layer-4-0"],
        )

    def test_escapes_unicode_inside_nested_text_material_json(self):
        script = Mock()
        script.materials.texts = [
            {"content": '{"text":"\u5b57\u5e55","styles":[]}'},
        ]

        escape_text_material_unicode(script)

        self.assertEqual(
            script.materials.texts[0]["content"],
            '{"text":"\\u5b57\\u5e55","styles":[]}',
        )


class MusicTrackExportTests(unittest.TestCase):
    @patch("create_jianying_draft.AudioSegment")
    @patch("create_jianying_draft.AudioMaterial")
    def test_loops_a_music_cue_and_applies_fades_only_at_its_edges(
        self, audio_material, audio_segment
    ):
        with TemporaryDirectory() as temporary_directory:
            source = Path(temporary_directory) / "music.wav"
            source.touch()
            first_segment = Mock(name="first")
            last_segment = Mock(name="last")
            audio_segment.side_effect = [first_segment, last_segment]
            script = Mock()
            tracks = [{
                "id": "music-1", "enabled": True,
                "cues": [{
                    "id": "cue-1", "sourceReference": str(source),
                    "sourceStartMs": 100, "sourceEndMs": 1_100,
                    "timelineStartMs": 500, "timelineEndMs": 2_500,
                    "loopEnabled": True, "volume": 0.4,
                    "fadeInMs": 120, "fadeOutMs": 180,
                }],
            }]

            add_music_tracks(script, tracks)

        script.add_track.assert_called_once_with(TrackType.audio, track_name="assembly-music-0")
        self.assertEqual(audio_segment.call_count, 2)
        first_segment.add_fade.assert_called_once_with(120_000, 0)
        last_segment.add_fade.assert_called_once_with(0, 180_000)
        self.assertEqual(
            [call.kwargs["track_name"] for call in script.add_segment.call_args_list],
            ["assembly-music-0", "assembly-music-0"],
        )


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


class DraftRollbackTests(unittest.TestCase):
    def test_rejects_a_draft_name_outside_the_selected_root(self):
        with TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            with self.assertRaises(RuntimeError):
                new_draft_path(root, "../outside")

    @patch("create_jianying_draft.DraftFolder")
    def test_removes_a_partial_directory_when_create_draft_fails(self, draft_folder):
        with TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            draft_name = "partial-draft"
            input_path = root / "input.json"
            input_path.write_text(
                json.dumps(
                    {
                        "inputFormatVersion": 2,
                        "operation": "createDraft",
                        "draftRoot": str(root),
                        "draftName": draft_name,
                        "draftRegistryPath": str(root / "root_meta_info.json"),
                        "clips": [],
                    }
                ),
                encoding="utf-8",
            )

            def create_draft(name, *_args, **_kwargs):
                (root / name).mkdir()
                raise OSError("template copy failed")

            draft_folder.return_value.create_draft.side_effect = create_draft
            with patch.object(sys, "argv", ["create_jianying_draft.py", str(input_path)]):
                with self.assertRaises(OSError):
                    main()

            self.assertFalse((root / draft_name).exists())

    @patch("create_jianying_draft.DraftFolder")
    def test_removes_a_new_draft_when_track_creation_fails(self, draft_folder):
        with TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            draft_name = "rollback-draft"
            input_path = root / "input.json"
            input_path.write_text(
                json.dumps(
                    {
                        "inputFormatVersion": 2,
                        "operation": "createDraft",
                        "draftRoot": str(root),
                        "draftName": draft_name,
                        "draftRegistryPath": str(root / "root_meta_info.json"),
                        "clips": [],
                        "textTracks": [
                            {
                                "id": "text-1",
                                "enabled": True,
                                "cues": [
                                    {
                                        "id": "cue-1",
                                        "startMs": 1_000,
                                        "endMs": 1_000,
                                        "text": "invalid",
                                    }
                                ],
                            }
                        ],
                        "musicTracks": [],
                    }
                ),
                encoding="utf-8",
            )
            script = Mock()

            def create_draft(name, *_args, **_kwargs):
                (root / name).mkdir()
                return script

            draft_folder.return_value.create_draft.side_effect = create_draft
            with patch.object(sys, "argv", ["create_jianying_draft.py", str(input_path)]):
                with self.assertRaises(RuntimeError):
                    main()

            self.assertFalse((root / draft_name).exists())


if __name__ == "__main__":
    unittest.main()

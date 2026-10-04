"""评测公开 fixture 的计分契约：空产物、越窗、音频时钟与未知金标。"""
import json
import csv
import tempfile
import unittest
from pathlib import Path

from metrics import score
from snapshot import write_json
from run import metric_stats

ROOT = Path(__file__).resolve().parents[2]


class MetricsContract(unittest.TestCase):
    def test_worst_success_is_failure_and_worst_risk_is_highest(self):
        self.assertEqual(metric_stats("localEditCompleted", [{"localEditCompleted": True}, {"localEditCompleted": False}])["worst"], 0)
        self.assertEqual(metric_stats("machineRiskSelectionsPrelabel", [{"machineRiskSelectionsPrelabel": 2}, {"machineRiskSelectionsPrelabel": 6}])["worst"], 6)

    def setUp(self):
        (ROOT / ".footage-eval").mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / ".footage-eval")
        self.base = Path(self.temp.name)
        self.run, self.snapshot = self.base / "run", self.base / "snapshot"
        self.run.mkdir()
        self.snapshot.mkdir()
        source = self.base / "source.mp4"
        source.write_bytes(b"metric fixture; not actual media")
        write_json(self.snapshot / "manifest.json", [{"assetId": "a", "source": str(source)}])
        write_json(self.snapshot / "analysis-original.json", [{"id": "a", "metadata_json": json.dumps({
            "durationMs": 10000, "sceneSegments": [{"id": "s", "startMs": 0, "endMs": 5000}]})}])
        self.case = {"targetMs": 4000, "genre": "promotion"}

    def tearDown(self):
        self.temp.cleanup()

    def put(self, end=4000, clock=0):
        board = {"beats": [{"id": "b"}], "shots": [{"assetId": "a", "beatId": "b", "sourceStartMs": 0,
                 "sourceEndMs": end, "durationMs": 4000}]}
        timeline = {"clips": [{"assetId": "a", "sourceStartMs": 0, "sourceEndMs": end, "timelineStartMs": 0, "timelineEndMs": 4000}],
                    "musicTracks": [{"enabled": True, "cues": [{"timelineEndMs": clock}]}] if clock else []}
        write_json(self.run / "evidence.json", {"storyboard_versions": [{"id": "s", "content_json": json.dumps(board)}],
                                               "timeline_versions": [{"id": "t", "content_json": json.dumps(timeline)}]})
        write_json(self.run / "result.json", {"status": "completed", "result": {"message": "完成"}})
        (self.run / "storyboard-provider-trace.jsonl").write_text(json.dumps({"phase": "Phase 1", "direction": "response", "body": {"beats": [{"id": "b"}]}}), encoding="utf-8")

    def test_empty_results_are_not_perfect_scores(self):
        write_json(self.run / "result.json", {"status": "completed", "result": {"message": "我先看看"}})
        value = score(self.run, self.snapshot, self.case)
        self.assertIsNone(value["referenceCoveragePct"])
        self.assertIsNone(value["hardRiskSelectionsGold"])
        self.assertEqual(value["goalReceiptMismatch"], 1)

    def test_selected_shots_do_not_make_planning_references(self):
        self.put()
        value = score(self.run, self.snapshot, self.case)
        self.assertEqual(value["referenceCoveragePct"], 100)
        self.assertEqual(value["planningReferenceCoveragePct"], 0)
        self.assertIsNone(value["hardRiskSelectionsGold"])

    def test_source_window_violation_is_not_covered(self):
        self.put(end=6000)
        value = score(self.run, self.snapshot, self.case)
        self.assertEqual(value["outOfWindowReferences"], 1)
        self.assertEqual(value["referenceCoveragePct"], 0)

    def test_audio_clock_reads_enabled_track_cues(self):
        self.put(clock=4500)
        value = score(self.run, self.snapshot, self.case)
        self.assertEqual(value["mainClockDeviationMs"], 500)

    def test_legacy_concatenated_trace_keeps_phase1(self):
        self.put()
        path = self.run / "storyboard-provider-trace.jsonl"
        path.write_text(path.read_text(encoding="utf-8") + json.dumps({"phase": "Phase 3", "direction": "response", "body": {}}) + "\n", encoding="utf-8")
        self.assertEqual(score(self.run, self.snapshot, self.case)["planningReferenceCoveragePct"], 0)

    def test_confirmed_risk_does_not_invent_best_window_label(self):
        self.put()
        row = {"asset_id": "a", "segment_id": "s", "start_ms": 0, "end_ms": 5000,
               "machine_risks": "未知/无正向风险词", "confirmed": "yes", "genre": "promotion",
               "gold_risks": "none", "best_start_ms": "", "best_end_ms": ""}
        with (self.snapshot / "gold.csv").open("w", encoding="utf-8-sig", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=list(row))
            writer.writeheader()
            writer.writerow(row)
        value = score(self.run, self.snapshot, self.case)
        self.assertEqual(value["hardRiskSelectionsGold"], 0)
        self.assertIsNone(value["acceptableBestWindowCountGold"])


if __name__ == "__main__":
    unittest.main()

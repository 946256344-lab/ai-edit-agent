"""独立策划评测契约：空结果不拿满分，ID/源窗必须分别核对。"""
import unittest
from planning_only import planning_metrics, shot_aspect


class PlanningMetricContract(unittest.TestCase):
    def test_empty_and_forged_references_are_not_coverage(self):
        self.assertEqual(shot_aspect({"aspectRatio": "9:16"}), "9:16")
        self.assertEqual(shot_aspect({"historicalMediaOptions": {"aspectRatio": "1:1"}}), "1:1")
        self.assertEqual(shot_aspect({}), "16:9")
        with self.assertRaises(ValueError):
            shot_aspect({"aspectRatio": "unsupported"})
        job = {"selection": "promotion", "eligibilityScope": "fixture", "eligibleEvidence": [
            {"id": "e1", "assetId": "a1", "segmentId": "s1", "range": {"startMs": 100, "endMs": 5000}}]}
        self.assertIsNone(planning_metrics({"result": {"status": "gap_only"}}, job)["planningReferenceCoveragePct"])
        ref = {"evidenceId": "e1", "assetId": "a1", "segmentId": "s1", "range": {"startMs": 100, "endMs": 4000}}
        section = {"claims": [{"reference": ref}], "alternatives": []}
        result = {"result": {"status": "limited", "proposal": {"sections": [section]}}}
        self.assertEqual(planning_metrics(result, job)["planningReferenceCoveragePct"], 100)
        ref["evidenceId"] = "forged"
        metric = planning_metrics(result, job)
        self.assertEqual(metric["illegalReferences"], 1)
        self.assertEqual(metric["planningReferenceCoveragePct"], 0)
        ref["evidenceId"] = "e1"
        ref["range"]["startMs"] = 0
        self.assertEqual(planning_metrics(result, job)["outOfWindowReferences"], 1)


if __name__ == "__main__":
    unittest.main()

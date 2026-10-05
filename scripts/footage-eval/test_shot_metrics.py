"""新增评测结果契约：空结果不能获得零违规或满分。"""
import tempfile
import json
import unittest
from pathlib import Path
from shot_metrics import score_shots


class ShotContract(unittest.TestCase):
    def test_missing_output_does_not_score_constraints_as_passed(self):
        output=Path(__file__).resolve().parents[2]/".footage-eval"
        output.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=output) as directory:
            metrics=score_shots(Path(directory))
        self.assertFalse(metrics["completeRefinement"])
        for key in ("duplicateAssets","similarReuse","cropOutside","actionTruncations","outOfWindow","crossHardCuts","eligibleSegments","eligibleBeforeRetry"):
            self.assertIsNone(metrics[key],key)

    def test_relation_negative_cannot_be_reused_after_endpoint_changes(self):
        output=Path(__file__).resolve().parents[2]/".footage-eval"
        output.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=output) as directory:
            directory=Path(directory)
            shots=[{"slot":i,"reference":{"evidenceId":str(i),"assetId":str(i),"range":{"startMs":0,"endMs":1000}}} for i in range(2)]
            pairs=[{"before":str(i),"after":str(j),"beforeRange":{"startMs":100,"endMs":900},"afterRange":{"startMs":0,"endMs":1000},"similar":"not_hit","sameScene":"not_hit","confidence":0.9} for i,j in [(0,1),(1,0)]]
            for name,value in [("combination.json",{"shots":shots}),("relations-input.json",{"pairs":pairs})]:
                (directory/name).write_text(json.dumps(value),encoding="utf-8")
            metrics=score_shots(directory)
        self.assertEqual(metrics["similarityUnknownPairs"],1)
        self.assertEqual(metrics["adjacentSceneUnknown"],1)


if __name__=="__main__":
    unittest.main()

"""冻结证据两次导出 + 历史产物/P5/计分回放；绝不声称重新生成了模型结果。"""
import csv
import json
import shutil
import subprocess
from pathlib import Path

from evidence_metrics import evidence_summary, read_rows
from metrics import score
from snapshot import DB_NAME, digest, write_json


def replay(root, baseline, snapshot, output, executable, version, gold=None):
    original_hashes = {p: digest(p) for p in [snapshot / DB_NAME, snapshot / "analysis-original.json", snapshot / "gold.csv"]}
    def export(target):
        subprocess.run([str(executable), "--export-evidence", str(snapshot / DB_NAME), str(target)], cwd=root, check=True)
    export(output / "segment-evidence.json")
    export(output / "segment-evidence-repeat.json")
    stable = digest(output / "segment-evidence.json") == digest(output / "segment-evidence-repeat.json")
    if not stable:
        raise RuntimeError("同一分析快照证据不稳定")
    contracts = json.loads((output / "segment-evidence.json").read_text(encoding="utf-8"))
    coverage = evidence_summary(contracts, read_rows(snapshot / "gold.csv"), read_rows(gold or snapshot / "evidence-gold.csv"))
    cases = json.loads((baseline / "cases.json").read_text(encoding="utf-8"))
    comparisons = []
    core_keys = ["produced", "status", "shots", "beats", "planningReferenceCoveragePct", "referenceCoveragePct",
                 "duplicateAssets", "sourceOverlapMs", "targetDeviationMs", "machineRiskSelectionsPrelabel",
                 "clarificationTurns", "receiptArtifactMismatches", "sourceWindowViolations", "pictureGapMs"]
    for case in cases:
        for repeat in range(1, 4):
            source = baseline / case["id"] / str(repeat)
            run = output / case["id"] / str(repeat)
            run.mkdir(parents=True)
            for p in source.iterdir():
                if p.is_file() and p.suffix in (".json", ".jsonl"):
                    shutil.copy2(p, run / p.name)
            for p in (source / "appdata" / "voiceovers").rglob("manifest.json"):
                copied = run / p.relative_to(source)
                copied.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(p, copied)
            shutil.copy2(output / "segment-evidence.json", run / "segment-evidence.json")
            subprocess.run([str(executable), "--validate-baseline", str(snapshot / DB_NAME), str(run / "evidence.json"), str(run / "p5-replay.json")], cwd=root, check=True)
            value = score(run, snapshot, case, evidence_gold_path=gold)
            prior = json.loads((source / "metrics.json").read_text(encoding="utf-8"))
            diffs = {k: {"baseline": prior.get(k), "replay": value.get(k)} for k in core_keys if prior.get(k) != value.get(k)}
            write_json(run / "contract-replay-metrics.json", value)
            artifacts_equal = digest(source / "evidence.json") == digest(run / "evidence.json")
            validation = json.loads((run / "p5-replay.json").read_text(encoding="utf-8"))
            comparisons.append({"caseId":case["id"], "repeat":repeat, "coreMetricsEqual":not diffs,
                                "metricDifferences":diffs, "savedArtifactsUnchanged":artifacts_equal, "validation":validation})
    unchanged = all(digest(p) == h for p, h in original_hashes.items())
    report = {"track":"frozen_analysis_and_saved_output_rule_replay", "liveGeneration":False,
              "code":version, "snapshot":str(snapshot), "baseline":str(baseline), "stableEvidence":stable,
              "evidenceSha256":digest(output / "segment-evidence.json"), "frozenFilesUnchanged":unchanged,
              "coverage":coverage, "comparisons":comparisons}
    write_json(output / "contract-replay-report.json", report)
    with (output / "evidence-gold-template.csv").open("w", encoding="utf-8-sig", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=["asset_id","segment_id","risk","state","start_ms","end_ms"])
        writer.writeheader()
        writer.writerows({"asset_id":e["assetId"],"segment_id":e["segmentId"],"risk":r,"state":"",
                         "start_ms":e["range"]["startMs"],"end_ms":e["range"]["endMs"]} for e in contracts for r in coverage["perRisk"])
    print(json.dumps({"stableEvidence":stable,"frozenFilesUnchanged":unchanged,"coverage":coverage,
                      "casesCompared":len(comparisons),"coreMetricsEqual":sum(c["coreMetricsEqual"] for c in comparisons),
                      "p5Passed":sum(v["passed"] for c in comparisons for v in c["validation"]["versions"]),
                      "p5Versions":sum(len(c["validation"]["versions"]) for c in comparisons)},ensure_ascii=False,indent=2))
    if not unchanged or not all(c["coreMetricsEqual"] and c["savedArtifactsUnchanged"] for c in comparisons):
        raise RuntimeError("冻结分析或历史产物计分发生变化，见报告")

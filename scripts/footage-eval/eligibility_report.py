"""只读冻结基线与真实复跑，统计入池硬风险、淘汰缺口和三体裁同片裁决。"""
import argparse
import collections
import json
import subprocess
from pathlib import Path
from metrics import trace_records


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def traces(run):
    records = []
    for path in sorted(run.glob("*storyboard-pool-trace.jsonl")):
        records.extend(dict(record, traceFile=path.name) for record in trace_records(path))
    return records


def summarize_run(run, judgments):
    promotion = {(d["assetId"], d["segmentId"]): d for d in judgments["decisions"] if d["genre"] == "promotion"}
    admitted_hits = 0
    unqualified = 0
    measured = False
    count = 0
    active_decisions = None
    trace_file = None
    guarded = legacy = guarded_hits = guarded_unqualified = 0
    for record in traces(run):
        if trace_file != record["traceFile"]:
            active_decisions = None
            trace_file = record["traceFile"]
        if record["phase"] == "Genre eligibility":
            active_decisions = {(d["assetId"], d["segmentId"]): d for d in record["body"]["decisions"]}
        if record["phase"] != "Phase 2":
            continue
        measured = True
        for candidate in record["body"].get("candidates", []):
            count += 1
            segment = candidate.get("segmentId") or "whole"
            members = [(active_decisions if active_decisions is not None else promotion).get((candidate["assetId"], member)) for member in segment.split("+")]
            hit = int(any(d and any(r["state"] == "hit" for r in d["reasons"]) for d in members))
            bad = int(any(d is None or d["status"] != "eligible" for d in members))
            admitted_hits += hit
            unqualified += bad
            if active_decisions is not None:
                guarded += 1
                guarded_hits += hit
                guarded_unqualified += bad
            else:
                legacy += 1
    decisions = [d for r in traces(run) if r["phase"] == "Genre eligibility" for d in r["body"]["decisions"]]
    reasons = collections.Counter(reason["code"] for d in decisions if d["status"] != "eligible" for reason in d["reasons"])
    failed = [r["body"]["error"] for r in traces(run) if r["phase"] == "Genre verification failure"]
    metrics = read(run / "metrics.json")
    return {"produced": metrics["produced"], "status": metrics["status"],
            "poolMeasured": measured, "poolAdmissions": count, "knownHardRiskAdmissions": admitted_hits,
            "gateAttempted": bool(decisions), "guardedPoolAdmissions": guarded,
            "legacyPoolAdmissions": legacy, "guardedHardRiskAdmissions": guarded_hits,
            "guardedUnqualifiedAdmissions": guarded_unqualified,
            "unqualifiedAdmissions": unqualified, "exclusionReasons": dict(reasons),
            "verificationFailures": dict(collections.Counter(failed)),
            "machineRiskSelectionsPrelabel": metrics["machineRiskSelectionsPrelabel"],
            "shots": metrics["shots"], "completedWithoutArtifact": metrics["completedWithoutArtifact"]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--frozen-judgments", type=Path, required=True)
    args = parser.parse_args()
    frozen = read(args.frozen_judgments)
    cases = read(args.output / "cases.json")
    report = {"policyVersion": frozen["policyVersion"], "liveGeneration": True,
              "goldMetrics": "N/A: no corrected human labels", "cases": {}}
    lines = ["# 体裁底线评测", "", "真实生成使用冻结分析；本入口暂按宣传。已命中硬风险是结构化事实口径，机器预标与人工金标分开。",
             "空产物的风险不能记作最终选镜满分；没有 Phase 2 的运行不算入池门已测。三体裁抽样是纯裁决回放，不是三体裁生成。", "",
             "| 用例 | 基线产出/3 → 本次 | 基线入池硬风险 → 本次 | 本次入池已测/3 | 最终机器预标均值：基线 → 本次 |",
             "|---|---|---|---|---|"]
    for case in cases:
        before, after = [], []
        for repeat in range(1, 4):
            run = args.output / case["id"] / str(repeat)
            target = run / "eligibility-judgments.json"
            subprocess.run([str(args.binary.resolve()), "--judge-eligibility", str((run / "segment-evidence.json").resolve()), str(target.resolve())], check=True)
            after.append(summarize_run(run, read(target)))
            before.append(summarize_run(args.baseline / case["id"] / str(repeat), frozen))
        def mean_risk(runs):
            measured = [r["machineRiskSelectionsPrelabel"] for r in runs if r["machineRiskSelectionsPrelabel"] is not None]
            return round(sum(measured) / len(measured), 3) if measured else None
        result = {"baseline": before, "current": after, "baselineMachinePrelabelMean": mean_risk(before), "currentMachinePrelabelMean": mean_risk(after)}
        report["cases"][case["id"]] = result
        lines.append(f"| {case['id']} | {sum(r['produced'] for r in before)} → {sum(r['produced'] for r in after)} | "
                     f"{sum(r['knownHardRiskAdmissions'] for r in before)} → {sum(r['knownHardRiskAdmissions'] for r in after)} | "
                     f"{sum(r['poolMeasured'] for r in after)} | {mean_risk(before)} → {mean_risk(after)} |")
    lines.extend(["", "## 无产物与淘汰原因", ""])
    for case, result in report["cases"].items():
        for index, run in enumerate(result["current"], 1):
            if not run["produced"]:
                lines.append(f"- {case} #{index}：status={run['status']}；淘汰={json.dumps(run['exclusionReasons'], ensure_ascii=False)}；核验失败={json.dumps(run['verificationFailures'], ensure_ascii=False)}。没有底线轨迹的失败不能归因于底线。")
    grouped = collections.defaultdict(list)
    for decision in frozen["decisions"]:
        grouped[(decision["assetId"], decision["segmentId"])].append(decision)
    # 从有正向风险的片段和其他片段各取样；无金标不写正确率。
    positive = [key for key, values in grouped.items() if any(r["state"] == "hit" for d in values for r in d["reasons"])]
    others = [key for key in grouped if key not in positive]
    samples = positive[:8] + others[:4]
    lines.extend(["", "## 三体裁同片裁决抽样（冻结旧分析，无关系上下文）", "", "| 资产 / 片段 | 宣传 | 叙事 | 花絮 | 宣传原因 |", "|---|---|---|---|---|"])
    for key in samples:
        values = {d["genre"]: d for d in grouped[key]}
        reasons = ", ".join(r["code"] for r in values["promotion"]["reasons"])
        lines.append(f"| {key[0]} / {key[1]} | {values['promotion']['status']} | {values['narrative']['status']} | {values['bts']['status']} | {reasons} |")
    report["frozenGenreCounts"] = dict(collections.Counter(f"{d['genre']}:{d['status']}" for d in frozen["decisions"]))
    current = [run for case in report["cases"].values() for run in case["current"]]
    report["guardedPoolAdmissions"] = sum(r["guardedPoolAdmissions"] for r in current)
    report["guardedHardRiskAdmissions"] = sum(r["guardedHardRiskAdmissions"] for r in current)
    report["guardedUnqualifiedAdmissions"] = sum(r["guardedUnqualifiedAdmissions"] for r in current)
    report["legacyPoolAdmissions"] = sum(r["legacyPoolAdmissions"] for r in current)
    (args.output / "eligibility-summary.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (args.output / "eligibility-report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(args.output / "eligibility-report.md")
    if report["guardedHardRiskAdmissions"] or report["guardedUnqualifiedAdmissions"]:
        raise RuntimeError("新生成入池底线失败，见 eligibility-summary.json")


if __name__ == "__main__":
    main()

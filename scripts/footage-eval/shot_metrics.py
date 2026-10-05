"""独立核对组合/精修事实；无结果不记零，采样机器证据和人工金标分开。"""
import collections
import json
import sqlite3
import statistics
from snapshot import DB_NAME, write_json


def read(path, default=None):
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else default


def score_shots(directory):
    result = read(directory / "shot-result.json", {"status": "not_executed"})
    combined = read(directory / "combination.json", {})
    refined = read(directory / "refined-shots.json", [])
    relation = read(directory / "relations-input.json", {})
    final_relation = read(directory / "final-relations.json")
    eligible = read(directory / "eligible-evidence.json", [])
    facts = {e["id"]: e for e in eligible}
    selected = combined.get("shots", [])
    pairs = {(p["before"], p["after"]): p for p in (final_relation or relation).get("pairs", [])}
    actual_ranges = {s["selected"]["slot"]: s["sourceRange"] for s in refined}
    def pair_fact(a, b):
        p = pairs.get((a["reference"]["evidenceId"], b["reference"]["evidenceId"]), {})
        if (p.get("beforeRange") != actual_ranges.get(a["slot"], a["reference"]["range"])
                or p.get("afterRange") != actual_ranges.get(b["slot"], b["reference"]["range"])
                or not isinstance(p.get("confidence"), (int, float)) or not 0 <= p["confidence"] <= 1):
            return {}
        return p
    metrics = {"status": result["status"], "selectedShots": len(selected), "refinedShots": len(refined),
               "completeRefinement": result["status"] == "refined" and bool(selected) and len(refined) == len(selected),
               "refinementCoveragePct": 100 * len(refined) / len(selected) if selected else None,
               "finalRelationErrors": result.get("finalRelationErrors", []),
               "eligibleSegments": len(eligible) if (directory / "eligible-evidence.json").exists() else None, "failedRefinements": len(result.get("refinementErrors", [])),
               "refinementErrors": result.get("refinementErrors", []), "error": result.get("error"),
               "duplicateAssets": None, "similarReuse": None, "similarityUnknownPairs": None,
               "adjacentSameScene": None, "adjacentSceneUnknown": None, "highlightsInWindowPct": None,
               "actionTruncations": None, "cropOutside": None, "cropUnknown": None,
               "outOfWindow": None, "crossHardCuts": None, "bestWindowGold": None,
               "narrativeRelationsGold": None}
    if selected:
        metrics["duplicateAssets"] = len(selected) - len({s["reference"]["assetId"] for s in selected})
        similar = unknown = 0
        for i, a in enumerate(selected):
            for b in selected[i + 1:]:
                states = [pair_fact(a,b).get("similar", "unknown"), pair_fact(b,a).get("similar", "unknown")]
                similar += "hit" in states
                unknown += "hit" not in states and "unknown" in states
        metrics.update(similarReuse=similar, similarityUnknownPairs=unknown,
                       adjacentSameScene=sum(pair_fact(a,b).get("sameScene") == "hit" for a, b in zip(selected, selected[1:])),
                       adjacentSceneUnknown=sum(pair_fact(a,b).get("sameScene", "unknown") == "unknown" for a, b in zip(selected, selected[1:])))
    if refined:
        c = sqlite3.connect(f"file:{(directory / 'appdata' / DB_NAME).as_posix()}?mode=ro", uri=True)
        outside = cross = highlight_total = highlight_in = actions_cut = action_total = crop_out = crop_unknown = 0
        for shot in refined:
            reference = shot["selected"]["reference"]
            start, end = shot["sourceRange"]["startMs"], shot["sourceRange"]["endMs"]
            outside += not reference["range"]["startMs"] <= start < end <= reference["range"]["endMs"]
            raw = c.execute("SELECT metadata_json FROM assets WHERE id=?", [reference["assetId"]]).fetchone()
            metadata = json.loads(raw[0])
            cross += sum(s["startMs"] < end and s["endMs"] > start for s in metadata.get("sceneSegments", [])) > 1
            highlights = shot["protectedHighlights"]
            highlight_total += len(highlights)
            highlight_in += sum(start <= t < end for t in highlights)
            actions_cut += sum(r["startMs"] < start or r["endMs"] > end for r in shot["protectedActions"])
            action_total += len(shot["protectedActions"])
            evidence = facts[reference["evidenceId"]]
            spans = shot["observation"]["subjectSpans"] + (evidence.get("relations") or {}).get("subjectSpans", [])
            spans = [s for s in spans if start <= s["timeMs"] < end]
            w, h = metadata.get("width"), metadata.get("height")
            aspect = read(directory / "job.json").get("aspectRatio", "16:9")
            ratio = {"16:9": 16 / 9, "9:16": 9 / 16, "1:1": 1}[aspect]
            if not w or not h or w / h < ratio - 0.0001:
                crop_unknown += 1
            else:
                fraction = min(1, ratio / (w / h))
                left = max(0, min(1 - fraction, shot["cropFocus"][0] - fraction / 2))
                crop_unknown += fraction < 0.9999 and not spans
                crop_out += any(s["left"] < left - 0.0001 or s["right"] > left + fraction + 0.0001 for s in spans)
        c.close()
        metrics.update(highlightsInWindowPct=100 * highlight_in / highlight_total if highlight_total else None,
                       highlightCount=highlight_total, actionCount=action_total, actionTruncations=actions_cut, cropOutside=crop_out,
                       cropUnknown=crop_unknown, outOfWindow=outside, crossHardCuts=cross)
    events = []
    path = directory / "storyboard-pool-trace.jsonl"
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            event = json.loads(line)
            if event.get("phase") == "Evidence verification retry":
                events.append(event["body"])
    recovered = {(e["assetId"], e["segmentId"]) for e in events if e["retrySuccess"]}
    recovered_eligible = sum((e["assetId"], e["segmentId"]) in recovered for e in eligible)
    verification = read(directory / "verification-result.json")
    preparation_failures = max(0, sum("Err" in r for r in verification.get("results", [])) - sum(bool(e["finalError"]) for e in events)) if verification else 0
    metrics.update(verificationRequests=len(verification.get("requests", [])) if verification else 0,
                   verificationPreparationFailures=preparation_failures,
                   verificationFirstFailures=sum(bool(e["firstError"]) for e in events) + preparation_failures,
                   verificationFinalFailures=sum(bool(e["finalError"]) for e in events) + preparation_failures,
                   verificationRetrySuccesses=sum(e["retrySuccess"] for e in events),
                   retryRecoveredEligibleSegments=recovered_eligible,
                   eligibleBeforeRetry=len(eligible) - recovered_eligible if (directory / "eligible-evidence.json").exists() else None,
                   eligibleAfterRetry=len(eligible) if (directory / "eligible-evidence.json").exists() else None)
    # 未跑核验保持 N/A，不能用 0 代表旧失败数已改善。
    if not verification:
        for key in ("verificationRequests", "verificationPreparationFailures", "verificationFirstFailures", "verificationFinalFailures", "verificationRetrySuccesses", "retryRecoveredEligibleSegments", "eligibleBeforeRetry", "eligibleAfterRetry"):
            metrics[key] = None
    write_json(directory / "shot-metrics.json", metrics)
    return metrics


def report_shots(output, results, baseline=None):
    measured = [v["shotRelations"] for v in results if v.get("shotRelations")]
    keys = ["duplicateAssets", "similarReuse", "similarityUnknownPairs", "adjacentSameScene", "adjacentSceneUnknown", "highlightsInWindowPct", "highlightCount", "actionCount", "actionTruncations", "cropOutside", "cropUnknown", "outOfWindow", "crossHardCuts", "verificationFirstFailures", "verificationFinalFailures", "verificationRetrySuccesses", "eligibleBeforeRetry", "eligibleAfterRetry"]
    aggregates = {}
    for key in keys:
        values = [v[key] for v in measured if v.get(key) is not None]
        aggregates[key] = {"mean": statistics.mean(values) if values else None,
                           "worst": (min(values) if key in {"highlightsInWindowPct", "eligibleBeforeRetry", "eligibleAfterRetry"} else max(values)) if values else None,
                           "measured": len(values), "total": sum(values) if values else None}
    summary = {"runs": len(results), "statuses": dict(collections.Counter(v["status"] for v in measured)),
               "completeRefinements": sum(v["completeRefinement"] for v in measured),
               "plans": sum(v["status"] in {"accepted", "limited"} for v in results),
               "limitedPlans": sum(v["status"] == "limited" for v in results), "metrics": aggregates,
               "baseline4b": {"plans": 24, "limitedPlans": 23, "limitedPlanPct": 23 / 24 * 100, "eligibleSegments": 25},
               "gold": "N/A: no human correction, industrial library only; machine sampled observations are not visual truth"}
    summary["limitedPlanPct"] = 100 * summary["limitedPlans"] / summary["plans"] if summary["plans"] else None
    if (output / "verification-audit.json").exists():
        summary["verificationAudit"] = read(output / "verification-audit.json")
    if baseline and baseline.exists():
        paired=[]
        for r in results:
            if r["caseId"] == "local-replace":
                continue  # 本任务仅跑 setup，旧基线 metrics 是 followup，不能同名混算。
            old=read(baseline/r["caseId"]/str(r["repeat"])/"metrics.json")
            if old:
                paired.append((old,r.get("shotRelations",{})))
        comparison={}
        for current,old_key in (("duplicateAssets","duplicateAssets"),("outOfWindow","finalSourceWindowViolations"),("crossHardCuts","crossHardCutSelections")):
            old=[o[old_key] for o,n in paired if o.get(old_key) is not None]
            new=[n[current] for o,n in paired if n.get(current) is not None]
            comparison[current]={"baselineMean":statistics.mean(old) if old else None,"baselineWorst":max(old) if old else None,"baselineMeasured":len(old),
                                 "currentMean":statistics.mean(new) if new else None,"currentWorst":max(new) if new else None,"currentMeasured":len(new)}
        summary["baselineComparison"]={"source":str(baseline),"commonRuns":len(paired),"metrics":comparison,
            "limitation":"baseline timeline versus current independently verified source-window outputs; local-replace followup excluded from setup comparison; unproduced runs remain N/A; visual similarity/action/crop not scored in old baseline"}
    write_json(output / "shot-summary.json", summary)
    lines = ["# 选镜关系与证据源窗评测", "", f"正式运行 {summary['runs']}；策划 {summary['plans']}；受限 {summary['limitedPlans']} ({summary['limitedPlanPct']}%)；全部镜头精修成功 {summary['completeRefinements']}。",
             f"状态：{summary['statuses']}", "", "任务 4b：24 份策划，23 份受限（95.83%），合格池 25。新一轮同时重做策划，不把比例差异单独归因为重试。",
             "", "| 指标 | 均值 | 最差 | 实测次数 | 总数 |", "|---|---:|---:|---:|---:|"]
    for key, value in aggregates.items():
        lines.append(f"| {key} | {value['mean']} | {value['worst']} | {value['measured']} | {value['total']} |")
    if summary.get("verificationAudit"):
        audit=summary["verificationAudit"]
        lines.extend(["", f"独立真实风险核验（冻结后供各例三次运行，不算 36 次核验）：{audit['requests']} 个请求，首轮失败 {audit['firstFailures']}，最终失败 {audit['finalFailures']}，重试成功 {audit['retrySuccesses']}，合格池 {audit['eligibleBeforeRetry']} → {audit['eligibleAfterRetry']}。"])
    if summary.get("baselineComparison"):
        lines.extend(["", "| 与旧基线同用例的指标 | 旧均值/最差/测量数 | 新均值/最差/测量数 |", "|---|---|---|"])
        for key,value in summary["baselineComparison"]["metrics"].items():
            lines.append(f"| {key} | {value['baselineMean']} / {value['baselineWorst']} / {value['baselineMeasured']} | {value['currentMean']} / {value['currentWorst']} / {value['currentMeasured']} |")
        lines.append("旧基线为时间线，新输出为源窗集合，未接成片；旧基线没有动作/裁切/真实相似指标，这些保持 N/A，不拿 sameSegmentReuseProxy 冒充真实画面相似。")
    lines.extend(["", "| 用例 | 运行 | 策划 | 合格池 | 镜头 | 精修覆盖 | 最终状态 |", "|---|---:|---|---:|---:|---:|---|"])
    for row in sorted(results,key=lambda r:(r["caseId"],r["repeat"])):
        s=row.get("shotRelations",{})
        lines.append(f"| {row['caseId']} | {row['repeat']} | {row['status']} | {s.get('eligibleSegments')} | {s.get('selectedShots')} | {s.get('refinementCoveragePct')} | {s.get('status')} |")
    lines.extend(["", "无镜头指标保持 N/A；失败/未精修镜头没有约束通过分。0 只说明实际产出的镜头满足机器事实，未知相似关系单列。动作、主体、高光的识别正确性与最佳源窗须人工金标；不证明成片、时钟、叙事/花絮正例或桌面/编辑器通过。"])
    (output / "shot-report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")

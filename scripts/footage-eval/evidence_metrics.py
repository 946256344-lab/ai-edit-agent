"""只计量结构化证据，不把风险覆盖率当识别准确率或体裁合格率。"""
import csv
from collections import Counter

PRIMARY = ("brand_logo", "exhibition", "out_of_focus", "shake", "clutter", "on_screen_text", "crowd")
PRELABELS = {"品牌标识": "brand_logo", "展会/展厅": "exhibition", "失焦": "out_of_focus", "抖动": "shake", "杂乱背景": "clutter"}


def intersects(a, b):
    return min(a["endMs"], b["endMs"]) > max(a["startMs"], b["startMs"])


def pair_state(observations, window):
    if any(o["state"] == "hit" and intersects(o["range"], window) for o in observations):
        return "hit"
    # 明确核验只补齐其覆盖窗；不把几个零散否定时段拼成全窗阴性。
    if any(o["state"] == "not_hit" and o.get("confidence") is not None and o["range"]["startMs"] <= window["startMs"]
           and o["range"]["endMs"] >= window["endMs"] for o in observations):
        return "not_hit"
    return "unknown"


def evidence_summary(contracts, prelabels=(), gold=()):
    states, primary_states, by_risk = Counter(), Counter(), {}
    index = {(e["assetId"], e["segmentId"]): e for e in contracts}
    for evidence in contracts:
        risks = {o["risk"] for o in evidence["risks"]}
        for risk in sorted(risks):
            state = pair_state([o for o in evidence["risks"] if o["risk"] == risk], evidence["range"])
            states[state] += 1
            by_risk.setdefault(risk, Counter())[state] += 1
            if risk in PRIMARY:
                primary_states[state] += 1
    total = sum(states.values())
    primary_total = sum(primary_states.values())
    prelabel_count = prelabel_covered = 0
    for row in prelabels:
        evidence = index.get((row["asset_id"], row["segment_id"]))
        if not evidence:
            continue
        for label in row.get("machine_risks", "").split("|"):
            if label not in PRELABELS:
                continue
            prelabel_count += 1
            risk = PRELABELS[label]
            if any(o["risk"] == risk and o["state"] == "hit" for o in evidence["risks"]):
                prelabel_covered += 1
    tp = fn = fp = tn = unknown = unknown_negative = 0
    for row in gold:
        # 专用原始风险事实金标，避免把 genre 的 hard-risk none 当全部风险阴性。
        if row.get("state") not in ("hit", "not_hit"):
            continue
        evidence = index.get((row.get("asset_id"), row.get("segment_id")))
        if not evidence:
            continue
        if row.get("risk") not in by_risk:
            raise ValueError("不认识的证据风险金标")
        window = {"startMs": int(row.get("start_ms") or evidence["range"]["startMs"]),
                  "endMs": int(row.get("end_ms") or evidence["range"]["endMs"])}
        if not evidence["range"]["startMs"] <= window["startMs"] < window["endMs"] <= evidence["range"]["endMs"]:
            raise ValueError("证据金标时段越界")
        predicted = pair_state([o for o in evidence["risks"] if o["risk"] == row["risk"]], window)
        unknown += predicted == "unknown"
        if row["state"] == "hit":
            tp += predicted == "hit"
            fn += predicted != "hit"  # 未知也未检出，同时单列未知数。
        else:
            fp += predicted == "hit"
            tn += predicted == "not_hit"
            unknown_negative += predicted == "unknown"
    return {"segments": len(contracts), "riskPairs": total, "states": dict(states),
            "knownRiskCoveragePct": (total - states["unknown"]) / total * 100 if total else None,
            "unknownPct": states["unknown"] / total * 100 if total else None,
            "primaryRiskPairs": primary_total, "primaryStates": dict(primary_states),
            "primaryUnknownPct": primary_states["unknown"] / primary_total * 100 if primary_total else None,
            "perRisk": {risk: dict(counts) for risk, counts in sorted(by_risk.items())},
            "machinePrelabelRiskPairs": prelabel_count, "machinePrelabelCoveredPairs": prelabel_covered,
            "machinePrelabelRiskCoveragePct": prelabel_covered / prelabel_count * 100 if prelabel_count else None,
            "goldPairs": tp + fn + fp + tn + unknown_negative, "unknownOnGold": unknown,
            "falseNegativePct": fn / (tp + fn) * 100 if tp + fn else None,
            "falsePositivePct": fp / (fp + tn + unknown_negative) * 100 if fp + tn + unknown_negative else None,
            "goldConfusion": {"tp": tp, "fn": fn, "fp": fp, "tn": tn, "unknownNegative": unknown_negative}}


def read_rows(path):
    if not path or not path.exists():
        return []
    with path.open(encoding="utf-8-sig", newline="") as stream:
        return list(csv.DictReader(stream))

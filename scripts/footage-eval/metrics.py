"""从真实回执/最终源窗计量；缺产物或缺金标返回 null，避免空结果得满分。"""
import csv
import json
import re
from collections import Counter
from pathlib import Path


def overlap(a, b):
    return max(0, min(a[1], b[1]) - max(a[0], b[0]))


def inside_selected_window(shot, metadata):
    start, end = shot.get("sourceStartMs", 0), shot.get("sourceEndMs", 0)
    if not 0 <= start < end <= metadata.get("durationMs", 0):
        return False
    windows = metadata.get("sceneSegments", [])
    ids = (shot.get("segmentId") or "").split("+")
    selected = [s for s in windows if s.get("id") in ids]
    if selected and len(selected) == len(ids):
        return min(s["startMs"] for s in selected) <= start < end <= max(s["endMs"] for s in selected)
    return not windows or any(s.get("startMs", 0) <= start < end <= s.get("endMs", 0) for s in windows)


def trace_records(path):
    """旧观测写入可能把两条 JSON 拼在一行；按完整对象恢复，损坏则明确失败。"""
    content = path.read_text(encoding="utf-8")
    decoder = json.JSONDecoder()
    offset, records = 0, []
    while offset < len(content):
        if content[offset].isspace():
            offset += 1
            continue
        value, offset = decoder.raw_decode(content, offset)
        records.append(value)
    return records


def read_csv(path):
    with Path(path).open(encoding="utf-8-sig", newline="") as stream:
        return list(csv.DictReader(stream))


def score(run, snapshot, case, gold_path=None):
    result_file = run / "result.json"
    result = json.loads(result_file.read_text(encoding="utf-8")) if result_file.exists() else {}
    evidence_path = run / "evidence.json"
    evidence = json.loads(evidence_path.read_text(encoding="utf-8")) if evidence_path.exists() else {}
    boards = evidence.get("storyboard_versions", [])
    timelines = evidence.get("timeline_versions", [])
    board = json.loads(boards[-1]["content_json"]) if boards else None
    timeline = json.loads(timelines[-1]["content_json"]) if timelines else None
    assets = {a["assetId"]: a for a in json.loads((snapshot / "manifest.json").read_text(encoding="utf-8"))}
    analysis = {a["id"]: json.loads(a["metadata_json"]) for a in json.loads((snapshot / "analysis-original.json").read_text(encoding="utf-8"))}
    beats = board.get("beats", []) if board else []
    phase1_path = run / "storyboard-provider-trace.jsonl"
    planning = []
    if phase1_path.exists():
        planning = [record["body"] for record in trace_records(phase1_path)
                    if record.get("phase") == "Phase 1" and record.get("direction") == "response"]
    if not planning and (run / "setup-storyboard-provider-trace.jsonl").exists():
        planning = [record["body"] for record in trace_records(run / "setup-storyboard-provider-trace.jsonl")
                    if record.get("phase") == "Phase 1" and record.get("direction") == "response"]
    plan = planning[-1] if planning else None
    plan_beats = plan.get("beats", []) if isinstance(plan, dict) else []
    plan_covered, plan_illegal = 0, 0
    for beat in plan_beats:
        refs = beat.get("references", [])
        if beat.get("assetId"):
            refs = refs + [beat]
        legal_refs = [ref for ref in refs if ref.get("assetId") in assets and Path(assets[ref["assetId"]]["source"]).is_file()]
        plan_illegal += len(refs) - len(legal_refs)
        plan_covered += bool(legal_refs)
    shots = board.get("shots", []) if board else []
    final = timeline.get("clips", []) if timeline else shots
    final = [s for s in final if s.get("clipKind", "source") == "source"]
    illegal, outside, covered = 0, 0, set()
    for shot in shots:
        asset = assets.get(shot.get("assetId"))
        start, end = shot.get("sourceStartMs", 0), shot.get("sourceEndMs", 0)
        if asset is None or not Path(asset["source"]).is_file():
            illegal += 1
            continue
        metadata = analysis[shot["assetId"]]
        if not inside_selected_window(shot, metadata):
            outside += 1
            continue
        covered.add(shot.get("beatId"))
    counts = Counter(s.get("assetId") for s in final)
    duplicate = sum(max(0, count - 1) for count in counts.values())
    source_overlap = sum(overlap((a.get("sourceStartMs", 0), a.get("sourceEndMs", 0)),
                                 (b.get("sourceStartMs", 0), b.get("sourceEndMs", 0)))
                         for i, a in enumerate(final) for b in final[i + 1:] if a.get("assetId") == b.get("assetId"))
    duration = max((s.get("timelineEndMs", 0) for s in final), default=0) if timeline else sum(s.get("durationMs", 0) for s in shots)
    # sceneSegment identity is a reproducible proxy, never called perceptual similarity.
    segments = []
    for shot in final:
        for segment in analysis.get(shot.get("assetId"), {}).get("sceneSegments", []):
            if overlap((shot.get("sourceStartMs", 0), shot.get("sourceEndMs", 0)), (segment["startMs"], segment["endMs"])):
                segments.append((shot["assetId"], segment.get("id")))
    similar_proxy = sum(max(0, count - 1) for count in Counter(segments).values())
    questions = int(result.get("status") == "needs_clarification")
    reply = result.get("result", {}).get("message", "")
    # 仅确定性事实：检查返回产物 ID 的存在性与版本号，文字承诺单独留机器预标。
    mismatch = 0
    receipt = result.get("result", {})
    for name, table in [("storyboard", boards), ("timeline", timelines)]:
        artifact = receipt.get(name)
        if artifact:
            matching = [v for v in table if v["id"] == artifact.get("id")]
            if not matching or (matching[0].get("task_version_number") or matching[0].get("version_number")) != artifact.get("versionNumber"):
                mismatch += 1
    preview = receipt.get("preview")
    if preview and not Path(preview.get("previewPath", "")).is_file():
        mismatch += 1
    clocks = {}
    if timeline:
        for name in ("voiceoverTracks", "musicTracks"):
            tracks = timeline.get(name, [])
            cues = [cue for track in tracks if track.get("enabled", True) for cue in track.get("cues", [])]
            clocks[name] = {"count": len(cues), "endMs": max((c.get("timelineEndMs", 0) for c in cues), default=0), "tracks": tracks}
    voice_end = clocks.get("voiceoverTracks", {}).get("endMs", 0)
    music_end = clocks.get("musicTracks", {}).get("endMs", 0)
    main_clock = voice_end or music_end or duration
    final_window_violations, crossed_cuts, timeline_gaps, previous_end = 0, 0, 0, 0
    for clip in final:
        metadata = analysis.get(clip.get("assetId"), {})
        shot = next((s for s in shots if s.get("orderIndex") == clip.get("shotIndex")), {})
        candidate = {**clip, "segmentId": shot.get("segmentId")}
        final_window_violations += not inside_selected_window(candidate, metadata)
        crossed_cuts += any(clip.get("sourceStartMs", 0) < s.get("startMs", 0) < clip.get("sourceEndMs", 0) for s in metadata.get("sceneSegments", []))
        if timeline:
            timeline_gaps += max(0, clip.get("timelineStartMs", 0) - previous_end)
            previous_end = max(previous_end, clip.get("timelineEndMs", 0))
    gold_file = Path(gold_path) if gold_path else snapshot / "gold.csv"
    labels = read_csv(gold_file) if gold_file.exists() else []
    confirmed = [g for g in labels if g.get("confirmed") == "yes" and g.get("genre") == ("promotion" if case["genre"] == "auto" else case["genre"])]
    selected_gold, gold_risk, best_acceptable, best_reviewed = 0, 0, 0, 0
    machine_risk, risk_samples = 0, []
    for shot in final:
        matches = [g for g in labels if g["asset_id"] == shot.get("assetId") and overlap(
            (shot.get("sourceStartMs", 0), shot.get("sourceEndMs", 0)), (int(g["start_ms"]), int(g["end_ms"])))]
        hits = [g for g in matches if g["machine_risks"] != "未知/无正向风险词"]
        if hits:
            machine_risk += 1
            risk_samples.append({"shot": shot, "prelabels": [g["machine_risks"] for g in hits]})
        golds = [g for g in matches if g in confirmed]
        confirmed_segments = {g["segment_id"] for g in golds if g.get("gold_risks", "").strip()}
        if golds and all(g["segment_id"] in confirmed_segments for g in matches):
            selected_gold += 1
        for g in golds:
            if g.get("gold_risks") and g.get("gold_risks") != "none" and overlap(
                (shot.get("sourceStartMs", 0), shot.get("sourceEndMs", 0)),
                (int(g.get("risk_start_ms") or g["start_ms"]), int(g.get("risk_end_ms") or g["end_ms"]))):
                gold_risk += 1
                break
        if any(g.get("best_start_ms") and g.get("best_end_ms") and int(g["best_start_ms"]) <= shot.get("sourceStartMs", 0) < shot.get("sourceEndMs", 0) <= int(g["best_end_ms"]) for g in golds):
            best_acceptable += 1
        if any(g.get("best_start_ms") and g.get("best_end_ms") for g in golds):
            best_reviewed += 1
    metrics = {"status": result.get("status", "process_failed"), "produced": bool(final), "beats": len(beats), "shots": len(final),
        "planningReferenceCoveragePct": plan_covered / len(plan_beats) * 100 if plan_beats else None,
        "planningIllegalReferences": plan_illegal if plan_beats else None,
        "referenceCoveragePct": 100 * len(covered.intersection({b["id"] for b in beats})) / len(beats) if beats else None,
        "illegalReferences": illegal if board else None, "outOfWindowReferences": outside if board else None,
        "finalSourceWindowViolations": final_window_violations if final else None,
        "crossHardCutSelections": crossed_cuts if final else None, "timelineGapMs": timeline_gaps if timeline else None,
        "duplicateAssets": duplicate if final else None, "sourceOverlapMs": source_overlap if final else None,
        "sameSegmentReuseProxy": similar_proxy if final else None,
        "durationMs": duration if final else None, "targetDeviationMs": abs(duration - case["targetMs"]) if final else None,
        "targetDeviationPct": abs(duration - case["targetMs"]) / case["targetMs"] * 100 if final else None,
        "mainClockDeviationMs": abs(duration - main_clock) if final else None,
        "clarificationTurns": questions if result else None, "unnecessaryQuestionCount": None,
        "questionTurnPrelabel": int(bool(questions or re.search(r"[？?]|(?:请|需要|等).{0,10}确认|确认.{0,10}(?:后|再)|先.{0,15}确认|(?:please|before).{0,30}confirm", reply, re.I))) if result else None,
        "receiptArtifactMismatches": mismatch if result else None, "replyFactMismatches": None,
        "completedWithoutArtifact": int(result.get("status") == "completed" and not final) if result else None,
        "goalReceiptMismatch": int(result.get("status") == "completed" and not final and not case.get("expectedMissing")) if result else None,
        "replyClaimPrelabel": re.findall(r"已.{0,12}(?:完成|生成|配音|音乐)|(?:completed|generated|added music|voiceover)", reply, flags=re.I),
        "machineRiskSelectionsPrelabel": machine_risk if final else None,
        "goldSelectedCoveragePct": selected_gold / len(final) * 100 if final and confirmed else None,
        "hardRiskSelectionsGold": gold_risk if final and selected_gold == len(final) else None,
        "acceptableBestWindowCountGold": best_acceptable if final and best_reviewed == len(final) else None,
        "bestWindowGoldCoveragePct": best_reviewed / len(final) * 100 if final and best_reviewed else None,
        "semanticDirectEvidenceCoverageGold": None, "genreAccuracyGold": None,
        "similarVisualReuseGold": None, "enoughQualifiedFootageGold": None,
        "riskSamples": risk_samples, "audioClocks": clocks, "response": reply,
        "goldStatus": "预标，待用户纠正" if not confirmed else "部分或全部已纠正，见覆盖率"}
    audio_first = []
    for manifest_path in (run / "appdata" / "voiceovers").rglob("manifest.json"):
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        audio_first.append({"manifest": str(manifest_path), **{key: manifest.get(key) for key in
            ["provider", "modelId", "voiceId", "voiceName", "durationMs", "status"]}})
    metrics["audioFirstPreparations"] = audio_first
    metrics["spokenDurationMs"] = audio_first[-1].get("durationMs") if audio_first else None
    metrics["spokenTargetDeviationMs"] = abs(metrics["spokenDurationMs"] - case["targetMs"]) if metrics["spokenDurationMs"] else None
    # 针对已生成结果的人工纠正；不把自由文本或机器 matchLevel 当作金标。
    review_path = run.parent.parent / "run-review.csv"
    if review_path.exists():
        reviews = read_csv(review_path)
        review = next((r for r in reviews if r["case_id"] == case.get("id") and r["repeat"] == run.name), {})
        for column, key in [("gold_unnecessary_questions", "unnecessaryQuestionCount"), ("gold_reply_fact_mismatches", "replyFactMismatches")]:
            if review.get(column, "").strip():
                metrics[key] = int(review[column])
        if review.get("gold_genre_correct") in ("yes", "no"):
            metrics["genreAccuracyGold"] = 100 if review["gold_genre_correct"] == "yes" else 0
        if review.get("gold_enough_qualified_footage") in ("yes", "no"):
            metrics["enoughQualifiedFootageGold"] = review["gold_enough_qualified_footage"] == "yes"
    selections_path = run.parent.parent / "selection-review.csv"
    if selections_path.exists() and final:
        selections = [r for r in read_csv(selections_path)
                      if r["case_id"] == case.get("id") and r["repeat"] == run.name]
        by_index = {r["shot_index"]: r for r in selections}
        reviewed = [by_index.get(str(c.get("shotIndex", c.get("orderIndex"))), {}) for c in final]
        if all(r.get("gold_hard_risk") in ("yes", "no") for r in reviewed):
            metrics["hardRiskSelectionsGold"] = sum(r["gold_hard_risk"] == "yes" for r in reviewed)
            metrics["goldSelectedCoveragePct"] = 100
        if all(r.get("gold_best_window") in ("yes", "no") for r in reviewed):
            metrics["acceptableBestWindowCountGold"] = sum(r["gold_best_window"] == "yes" for r in reviewed)
            metrics["bestWindowGoldCoveragePct"] = 100
        if all(r.get("gold_direct_support") in ("yes", "no") for r in reviewed) and beats:
            supported = {r.get("beat_id") for r in reviewed if r["gold_direct_support"] == "yes"}
            metrics["semanticDirectEvidenceCoverageGold"] = len(supported.intersection({b["id"] for b in beats})) / len(beats) * 100
        if all(r.get("gold_relation_violation") in ("yes", "no") for r in reviewed):
            metrics["relationViolationCountGold"] = sum(r["gold_relation_violation"] == "yes" for r in reviewed)
    if any(metrics.get(k) is not None for k in ["hardRiskSelectionsGold", "acceptableBestWindowCountGold", "semanticDirectEvidenceCoverageGold", "genreAccuracyGold", "replyFactMismatches", "unnecessaryQuestionCount"]):
        metrics["goldStatus"] = "部分或全部已纠正，各指标仍按独立覆盖率判断"
    return metrics

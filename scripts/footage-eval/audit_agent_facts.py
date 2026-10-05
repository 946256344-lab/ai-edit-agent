"""独立核对任务 7 的固定回复事实与执行回执；不判断画面语义，不写基线目录。"""
import argparse
import json
import re
from pathlib import Path


def read(path):
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def audit_run(directory, case, expect_genre=True):
    metrics = read(directory / "metrics.json")
    result = read(directory / "result.json")
    outcome = result.get("result") or {}
    evidence = read(directory / "evidence.json")
    reply = outcome.get("message", "")
    board, timeline = outcome.get("storyboard") or {}, outcome.get("timeline") or {}
    mismatches = []
    persisted = {}
    for name, artifact in [("storyboard_versions", board), ("timeline_versions", timeline)]:
        record = next((row for row in evidence.get(name, []) if row["id"] == artifact.get("id")), None)
        persisted[name] = json.loads(record["content_json"]) if record else {}
        if artifact and (not record or (record.get("task_version_number") or record["version_number"]) != artifact.get("versionNumber")):
            mismatches.append(name + "_receipt")
        if name == "timeline_versions" and record:
            for field in ["clips", "textTracks", "voiceoverTracks", "musicTracks"]:
                if artifact.get(field, []) != persisted[name].get(field, []):
                    mismatches.append(field + "_receipt")
    for key, artifact, pattern in [
        ("storyboard_versions", board, r"(?:分镜|故事[版板]|Storyboard)\s*v(\d+)"),
        ("timeline_versions", timeline, r"(?:时间线|timeline)\s*v(\d+)")]:
        versions = [int(x) for x in re.findall(pattern, reply, re.I)]
        if versions and (any(v != artifact.get("versionNumber") for v in versions)
                         or not any(row["id"] == artifact.get("id") for row in evidence.get(key, []))):
            mismatches.append(key)
    stored_timeline = persisted["timeline_versions"]
    picture = [c for c in stored_timeline.get("clips", []) if c.get("clipKind") == "source"]
    counts = re.findall(r"共\s*(\d+)\s*个镜头|saved with (\d+) shots", reply, re.I)
    if any(int(zh or en) != len(picture) for zh, en in counts):
        mismatches.append("shots")
    for label, key in [("配音|Voiceover", "voiceoverTracks"), ("字幕|subtitles", "textTracks"), ("BGM", "musicTracks")]:
        claims = re.findall(r"(?:" + label + r")[：:]\s*(有|无|yes|no)(?:[。；;.\s]|$)", reply, re.I)
        actual = any(t.get("enabled") and t.get("cues") and (key != "textTracks" or t.get("role") == "subtitle")
                     for t in stored_timeline.get(key, []))
        if any((x.lower() in ("有", "yes")) != actual for x in claims):
            mismatches.append(key)
    preview_claim = re.search(r"已(?:重新)?生成.{0,16}预览|预览.{0,10}(?:已生成|已渲染|渲染成功)|The preview was generated", reply)
    preview = outcome.get("preview") or {}
    if preview_claim and (not preview or not Path(preview.get("previewPath", "")).is_file()
                          or preview.get("timelineVersionId") != timeline.get("id")):
        mismatches.append("preview")
    draft_claim = "编辑器交付物已创建" in reply or "The editor deliverable was created" in reply
    draft = outcome.get("jianyingDraft") or {}
    if draft_claim and not Path(draft.get("draftContentPath", "")).is_file():
        mismatches.append("delivery")
    # 只查字段/工具/ID 等明确内部标记；语义正确率仍需人工金标。
    leak = bool(re.search(r"mediaOptions|appliedMedia|responseInstruction|本轮自动添加选项|system prompt|input_text|</?think|</?analysis|"
                          r"(?:generate_storyboard|reselect_shots|refine_shot_ranges|search_assets|search_asset_segments|"
                          r"replace_text_tracks|set_transitions)|\b[0-9a-f]{8}-[0-9a-f-]{27}\b|(?<![A-Za-z0-9])[A-Za-z]:[\\/]", reply, re.I))
    answer_writes = case.get("answerOnly", False) and any(evidence.get(key) for key in ["storyboard_versions", "timeline_versions"])
    if answer_writes:
        mismatches.append("answer_created_artifact")
    genre_checks = [json.loads(row["input_json"]).get("mediaOptions", {}).get("genre") == case.get("genre")
                    for row in evidence.get("agent_tasks", []) if row.get("tool_name") == "agent_loop"]
    if board and not case.get("answerOnly"):
        genre_checks.append(persisted["storyboard_versions"].get("mediaOptions", {}).get("genre") == case.get("genre"))
    setup = read(directory / "setup-evidence.json")
    local_changed = None
    if setup.get("timeline_versions") and evidence.get("timeline_versions"):
        before = json.loads(setup["timeline_versions"][-1]["content_json"])
        after = json.loads(evidence["timeline_versions"][-1]["content_json"])
        identity = lambda content: [(c.get("assetId"), c.get("sourceStartMs"), c.get("sourceEndMs"))
                                    for c in content.get("clips", [])[:1]]
        local_changed = (identity(before) != identity(after)
                         and before.get("clips", [])[1:] == after.get("clips", [])[1:]
                         and all(before.get(k) == after.get(k) for k in ["musicTracks", "voiceoverTracks", "textTracks"]))
    return {"caseId": case["id"], "repeat": int(directory.name), "status": result.get("status", "process_failed"),
            "completedWithoutArtifact": metrics.get("completedWithoutArtifact"),
            "localEditCompleted": metrics.get("localEditCompleted"),
            "localEditFrozenRemainder": metrics.get("localEditFrozenRemainder"),
            "localEditFrozenAudio": metrics.get("localEditFrozenAudio"),
            "localPictureChangedWithFrozenRemainder": local_changed,
            "setupBlocked": (directory / "followup-blocked.json").exists(),
            "genreSnapshotMismatch": sum(not check for check in genre_checks) if genre_checks and expect_genre else None,
            "searchAssetArgumentFailures": sum(row["tool_name"] == "search_assets" and row.get("error_code") == "invalid_arguments"
                                               for row in evidence.get("agent_run_steps", [])) if evidence else None,
            "receiptArtifactMismatches": metrics.get("receiptArtifactMismatches"),
            "fixedReplyFactMismatches": len(mismatches) if outcome else None,
            "mismatchCategories": mismatches, "internalReplyLeak": leak if outcome else None,
            "questionTurnPrelabel": metrics.get("questionTurnPrelabel"), "answerCreatedArtifact": bool(answer_writes),
            "windows": result.get("isolation", {}).get("windows")}


def collect(output, cases, expect_genre=True):
    return [audit_run(output / c["id"] / str(n), c, expect_genre) for c in cases for n in range(1, 4)]


def summarize(rows):
    keys = ["completedWithoutArtifact", "receiptArtifactMismatches", "fixedReplyFactMismatches",
            "internalReplyLeak", "questionTurnPrelabel", "answerCreatedArtifact", "genreSnapshotMismatch", "searchAssetArgumentFailures"]
    summary = {k: {"count": sum(r[k] for r in rows if r.get(k) is not None),
                   "measuredRuns": sum(r.get(k) is not None for r in rows)} for k in keys}
    for case, key in [("local-replace", "localReplace"), ("local-refine", "localRefine")]:
        local = [r for r in rows if r["caseId"] == case]
        summary[key] = {"successes": sum(bool(r["localEditCompleted"]) for r in local), "runs": len(local),
                               "actualPictureChanges": sum(bool(r["localPictureChangedWithFrozenRemainder"]) for r in local),
                               "executedRuns": sum(r["localPictureChangedWithFrozenRemainder"] is not None for r in local),
                               "setupBlocked": sum(r["setupBlocked"] for r in local)}
    summary["statuses"] = {s: sum(r["status"] == s for r in rows) for s in sorted({r["status"] for r in rows})}
    return summary


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    args = parser.parse_args()
    cases = read(args.output / "cases.json")
    comparable = [c for c in cases if c["id"] in {"history-1", "history-2", "history-3", "local-replace"}]
    baseline, delivery = collect(args.baseline, comparable, expect_genre=False), collect(args.output, cases)
    comparison = {"scope": "版本、镜头数、音轨、当前版本预览/交付的固定事实；不含画面语义、最佳源窗或所有自由文本声明",
                  "questionCounting": "questionTurnPrelabel 是机器预标；是否无必要仍须逐轮审阅，不能将其自动当作金标",
                  "baseline": summarize(baseline), "deliveryComparable": summarize([r for r in delivery if r["caseId"] in {c["id"] for c in comparable}]),
                  "deliveryAll": summarize(delivery), "baselineRuns": baseline, "deliveryRuns": delivery}
    (args.output / "agent-facts-audit.json").write_text(json.dumps(comparison, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({k: v for k, v in comparison.items() if k not in ("baselineRuns", "deliveryRuns")}, ensure_ascii=False))


if __name__ == "__main__":
    main()

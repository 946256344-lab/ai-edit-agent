"""一条命令冻结/复跑真实 Agent，三次独立运行并输出报告；不启动 GUI。"""
import argparse
import csv
import concurrent.futures
import datetime
import hashlib
import json
import os
import shutil
import sqlite3
import statistics
import subprocess
import sys
import time
from pathlib import Path

from metrics import read_csv, score
from snapshot import DB_NAME, connect, digest, freeze, rows, write_json

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).resolve().parent
TABLES = ["storyboard_versions", "timeline_versions", "storyboard_recommendations", "agent_tasks",
          "messages", "conversations", "editing_tasks", "agent_run_steps", "agent_diagnostics", "operation_logs",
          "pending_clarifications", "task_state_snapshots", "pending_task_routes", "task_route_receipts"]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def code_version():
    files = set(git("ls-files", "src-tauri", "scripts/footage-eval").decode().splitlines())
    files.update(git("ls-files", "--others", "--exclude-standard", "src-tauri", "scripts/footage-eval").decode().splitlines())
    manifest = {name: digest(ROOT / name) for name in sorted(files) if (ROOT / name).is_file()}
    return {"commit": git("rev-parse", "HEAD").decode().strip(), "branch": git("branch", "--show-current").decode().strip(),
            "worktreeDiffSha256": hashlib.sha256(git("diff", "--binary", "HEAD")).hexdigest(),
            "sourceTreeSha256": hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest(),
            "sourceFiles": manifest, "dirty": bool(git("status", "--porcelain")), "recipeVersion": "existing-production-no-new-recipe"}


def export_evidence(run):
    c = connect(run / "appdata" / DB_NAME)
    evidence = {table: rows(c, "SELECT * FROM " + table + " ORDER BY rowid") for table in TABLES if table not in ("task_state_snapshots", "pending_task_routes", "task_route_receipts")}
    evidence["analysisAfter"] = rows(c, "SELECT id, metadata_json FROM assets WHERE project_id=(SELECT project_id FROM agent_tasks LIMIT 1) ORDER BY id")
    c.close()
    write_json(run / "evidence.json", evidence)
    return evidence


def export_segment_contracts(executable, database, output):
    subprocess.run([str(executable), "--export-evidence", str(database), str(output)], cwd=ROOT, check=True)



def add_task(run, project, case, phase, storyboard=None, timeline=None):
    c = connect(run / "appdata" / DB_NAME)
    now = int(time.time() * 1000)
    task = "eval-agent-" + phase
    editing = "eval-edit"
    conversation = "eval-conversation"
    if phase == "main":
        c.execute("INSERT INTO editing_tasks(id,project_id,title,brief,created_at,updated_at) VALUES(?,?,?,'',?,?)", (editing, project, case["id"], now, now))
        c.execute("INSERT INTO conversations(id,project_id,editing_task_id,title,summary,status,created_at,updated_at) VALUES(?,?,?,?,'','ready',?,?)", (conversation, project, editing, case["id"], now, now))
    request = case["request"] if phase == "main" else case["followup"]
    options = {"aspectRatio": "16:9", "bgm": case["bgm"], "voiceover": case["voiceover"], "subtitles": case["voiceover"]}
    inputs = {"mediaOptions": options, "userMessageId": "eval-user-" + phase, "requestLength": len(request),
              "storyboardVersionId": storyboard, "timelineVersionId": timeline, "uiLocale": case.get("uiLocale", "zh-CN")}
    c.execute("INSERT INTO messages(id,conversation_id,role,content,created_at) VALUES(?,?,'user',?,?)", (inputs["userMessageId"], conversation, request, now))
    c.execute("INSERT INTO agent_tasks(id,project_id,editing_task_id,conversation_id,tool_name,status,input_json,created_at,updated_at) VALUES(?,?,?,?,'agent_loop','queued',?,?,?)", (task, project, editing, conversation, json.dumps(inputs), now, now))
    c.commit()
    c.close()
    job = {"projectId": project, "editingTaskId": editing, "conversationId": conversation, "agentTaskId": task,
           "request": request, "storyboardVersionId": storyboard, "timelineVersionId": timeline,
           "case": case, "mediaOptions": options}
    write_json(run / "job.json", job)


def prepare(run, snapshot, project, case):
    (run / "appdata").mkdir(parents=True, exist_ok=False)
    shutil.copy2(snapshot / DB_NAME, run / "appdata" / DB_NAME)
    c = connect(run / "appdata" / DB_NAME)
    c.execute("PRAGMA foreign_keys=OFF")
    for table in TABLES:
        c.execute("DELETE FROM " + table)
    c.execute("DELETE FROM project_libraries WHERE project_id=?", (project,))
    # 隔离库范围到要求的 94 条视频 + 原有音频。其余源不参与候选召回。
    c.execute("DELETE FROM assets WHERE project_id<>?", (project,))
    c.execute("DELETE FROM asset_segment_embeddings WHERE asset_id NOT IN (SELECT id FROM assets)")
    c.commit()
    inventory = rows(c, "SELECT id,kind,analysis_status FROM assets WHERE project_id=? ORDER BY id", (project,))
    settings = rows(c, "SELECT settings_json FROM projects WHERE id=?", (project,))
    c.close()
    write_json(run / "initial-state.json", {"inventory": inventory, "usageCounts": {}, "artifacts": {},
        "projectSettings": settings, "sourceDatabaseSha256": digest(snapshot / DB_NAME),
        "constraints": ["94 industrial videos + original audio only", "no external editor delivery", "no card WebView", "no reanalysis tools", "read-only custom provider credentials"]})
    weights = snapshot / "runtime-models"
    if weights.exists():
        shutil.copytree(weights, run / "appdata" / "runtime-models", copy_function=os.link)
    add_task(run, project, case, "main")


def run_one(executable, run, snapshot, project, case, repeat, evidence_gold=None):
    prepare(run, snapshot, project, case)
    started = time.monotonic()
    # 每例独立进程，Tauri / ONNX / 新鲜度缓存不跨用例；工具内部并发保持生产行为。
    for phase in ("main", "followup") if case.get("followup") else ("main",):
        if phase == "followup":
            evidence = export_evidence(run)
            boards, timelines = evidence["storyboard_versions"], evidence["timeline_versions"]
            if not boards or not timelines:
                write_json(run / "followup-blocked.json", {"reason": "setup_did_not_produce_storyboard_and_timeline"})
                break
            shutil.copy2(run / "evidence.json", run / "setup-evidence.json")
            for name in ["result.json", "native-provider-full-trace.jsonl", "storyboard-provider-trace.jsonl", "storyboard-pool-trace.jsonl"]:
                if (run / name).exists():
                    (run / name).rename(run / ("setup-" + name))
            add_task(run, project, case, phase, boards[-1]["id"], timelines[-1]["id"])
        with (run / (phase + "-process.log")).open("w", encoding="utf-8") as stream:
            try:
                completed = subprocess.run([str(executable), str(run / "job.json")], cwd=ROOT,
                                           stdout=stream, stderr=stream, timeout=2400)
                exit_code = completed.returncode
            except subprocess.TimeoutExpired:
                exit_code = "timeout"
        write_json(run / (phase + "-process.json"), {"exitCode": exit_code})
    export_evidence(run)
    export_segment_contracts(executable, run / "appdata" / DB_NAME, run / "segment-evidence.json")
    metrics = score(run, snapshot, case, evidence_gold_path=evidence_gold)
    metrics.update({"caseId": case["id"], "repeat": repeat, "elapsedSeconds": time.monotonic() - started})
    if case.get("followup"):
        add_local_metrics(run, metrics)
    write_json(run / "metrics.json", metrics)
    print(f"{case['id']} #{repeat}: {metrics['status']} shots={metrics['shots']}", flush=True)
    return metrics


def add_local_metrics(run, metrics):
    if (run / "setup-evidence.json").exists():
        before = json.loads((run / "setup-evidence.json").read_text(encoding="utf-8"))
        after = json.loads((run / "evidence.json").read_text(encoding="utf-8"))
        old = json.loads(before["timeline_versions"][-1]["content_json"])
        new = json.loads(after["timeline_versions"][-1]["content_json"])
        metrics["localEditFrozenRemainder"] = old.get("clips", [])[1:] == new.get("clips", [])[1:]
        metrics["localEditFrozenAudio"] = all(old.get(k) == new.get(k) for k in ["musicTracks", "voiceoverTracks"])
        metrics["localEditChangedRequestedShot"] = old.get("clips", [])[:1] != new.get("clips", [])[:1]
        metrics["localEditCompleted"] = metrics["status"] == "completed" and metrics["localEditChangedRequestedShot"]
    else:
        metrics.update({"localEditFrozenRemainder": None, "localEditFrozenAudio": None,
                        "localEditChangedRequestedShot": None, "localEditCompleted": False})


def resolve_cases(snapshot):
    cases = json.loads((SCRIPT / "cases.json").read_text(encoding="utf-8"))
    history = json.loads((snapshot / "history.json").read_text(encoding="utf-8"))
    for case in cases:
        source = case.get("historyMessageId")
        if not source:
            case["provenance"] = "新基线，非历史复现"
            continue
        message = next((m for m in history["messages"] if m["id"] == source), None)
        task = next((t for t in history["tasks"] if json.loads(t["input_json"]).get("userMessageId") == source), None)
        if message and task:
            case["request"] = message["content"]
            options = json.loads(task["input_json"])["mediaOptions"]
            case.update({"voiceover": options["voiceover"], "bgm": options["bgm"], "historicalTaskId": task["id"],
                         "historicalCreatedAt": message["created_at"], "historicalMediaOptions": options,
                         "provenance": "09-27 历史输入复现；分析快照为当前版本，非历史环境完整复刻"})
        else:
            case["provenance"] = "新基线，非历史复现；未找到原始消息/开关"
    return cases


def metric_stats(key, runs):
    values = [r[key] for r in runs if isinstance(r.get(key), (int, float))]
    higher_is_better = key.endswith("CoveragePct") or key in {
        "semanticDirectEvidenceCoverageGold", "genreAccuracyGold", "acceptableBestWindowCountGold",
        "localEditCompleted", "localEditFrozenRemainder", "localEditFrozenAudio"}
    return {"mean": statistics.mean(values) if values else None,
            "worst": (min(values) if higher_is_better else max(values)) if values else None,
            "measuredRuns": len(values), "missingRuns": len(runs) - len(values)}


def report(output, cases, results, snapshot, version):
    keys = ["planningReferenceCoveragePct", "planningIllegalReferences", "referenceCoveragePct", "illegalReferences", "outOfWindowReferences", "duplicateAssets", "sourceOverlapMs",
            "finalSourceWindowViolations", "crossHardCutSelections", "timelineGapMs", "goalReceiptMismatch", "questionTurnPrelabel",
            "completedWithoutArtifact", "unnecessaryQuestionCount", "replyFactMismatches", "hardRiskSelectionsGold", "semanticDirectEvidenceCoverageGold", "genreAccuracyGold",
            "goldSelectedCoveragePct", "acceptableBestWindowCountGold", "bestWindowGoldCoveragePct", "relationViolationCountGold",
            "spokenDurationMs", "spokenTargetDeviationMs", "localEditCompleted", "localEditFrozenRemainder", "localEditFrozenAudio",
            "sameSegmentReuseProxy", "targetDeviationMs", "targetDeviationPct", "mainClockDeviationMs", "clarificationTurns",
            "receiptArtifactMismatches", "machineRiskSelectionsPrelabel"]
    summary = {}
    lines = ["# 选镜 / 策划基线：2026-10-01", "", "真实模型评测；冻结已有分析。每例 3 次，所有空产物指标记 N/A，不能算通过。",
             "重新导入分析未运行，作为独立轨道；工业库不能证明三体裁可用。", "",
             f"代码 HEAD：`{version['commit']}`；工作区 diff SHA256：`{version['worktreeDiffSha256']}`；源码树 SHA256：`{version['sourceTreeSha256']}`。",
             f"输出目录：`{output}`。", f"快照目录：`{snapshot}`。", "",
             "隔离限制：94 条工业视频 + 原有音频；不刷新登录凭据，不生成外部编辑器交付，不启动卡片 WebView，不重新分析。",
             "阶段请求/响应、候选池、计划、时间线、音频轨及回执见每次运行的 JSON/JSONL；没有产生的阶段保留缺失。", "",
             "机器风险、回复事实、必要性和体裁判定均为预标，待用户纠正；未纠正不能称为金标分数。", "",
             "| 用例 | 来源 | 产出/3 | 状态 | 策划引用均值/最差 % | 最终拍覆盖均值/最差 % | 复用均值/最差 | 重叠均值/最差 ms | 时长偏差均值/最差 ms | 澄清轮数均值/最差 | 回执 ID/版本不一致均值/最差 | 风险预标均值/最差 |",
             "|---|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for case in cases:
        runs = [r for r in results if r["caseId"] == case["id"]]
        stats = {}
        for key in keys:
            stats[key] = metric_stats(key, runs)
        summary[case["id"]] = {"stats": stats, "produced": sum(r["produced"] for r in runs), "runs": runs}
        def display(key):
            stat = stats[key]
            return "N/A" if stat["mean"] is None else f"{stat['mean']:.2f}/{stat['worst']:.2f} ({stat['measuredRuns']}/3)"
        statuses = ", ".join(sorted({r["status"] for r in runs}))
        columns = [case["id"], "历史输入" if "历史输入复现" in case["provenance"] else "新基线", str(sum(r["produced"] for r in runs)), statuses,
                   *(display(k) for k in ["planningReferenceCoveragePct", "referenceCoveragePct", "duplicateAssets", "sourceOverlapMs", "targetDeviationMs", "clarificationTurns", "receiptArtifactMismatches", "machineRiskSelectionsPrelabel"])]
        lines.append("| " + " | ".join(columns) + " |")
    contract_runs = [r["evidenceContract"] for r in results if "evidenceContract" in r]
    if contract_runs:
        lines.extend(["", "## 片段证据契约", "", "风险事实覆盖不是识别正确率或体裁合格率；未知不算安全。", "",
                      "| 契约指标 | 均值 | 最差 |", "|---|---:|---:|"])
        for key, label in [("knownRiskCoveragePct", "非未知风险覆盖 %"), ("unknownPct", "未知比例 %"),
                           ("primaryUnknownPct", "七项核心风险未知 %"), ("machinePrelabelRiskCoveragePct", "旧机器正向预标覆盖 %"),
                           ("falseNegativePct", "漏检率金标 %"), ("falsePositivePct", "误杀率金标 %")]:
            values = [r[key] for r in contract_runs if r[key] is not None]
            worst = (min(values) if "Coverage" in key else max(values)) if values else None
            lines.append(f"| {label} | {statistics.mean(values):.2f} | {worst:.2f} |" if values else f"| {label} | N/A | N/A |")
    lines.extend(["", "## 固定输入与关键样例", ""])
    for case in cases:
        lines.extend([f"### {case['id']}", "", f"{case['provenance']}；目标 {case['targetMs']} ms；配音 {case['voiceover']}；BGM {case['bgm']}；体裁意图 {case['genre']}。", "", case["request"], ""])
        runs = summary[case["id"]]["runs"]
        if runs:
            sample = max(runs, key=lambda r: r.get("targetDeviationMs") or 0)
            lines.append(f"关键回执：{sample['status']}；{sample.get('response', '')[:500]}\n")
            lines.append(f"风险预标样例：`{json.dumps(sample.get('riskSamples', [])[:1], ensure_ascii=False)[:1600]}`\n")
    lines.extend(["## 未验证与纠正入口", "", "硬淘汰入选数、语义直证、最佳区间、体裁/关系正确率、识别漏检/误杀、素材是否足够均缺人工金标，记 N/A。",
                  "无必要反问和自然语言事实一致性只有澄清状态/回复关键词预标，未作人工结论。回执 ID/版本一致性是可自动核对的子集。",
                  "修改 snapshot/gold.csv 的人工列并用 --rescore --gold 重新计分；详细定义见 docs/evaluation/README.md。"])
    write_json(output / "summary.json", summary)
    review = []
    old_reviews = {}
    if (output / "run-review.csv").exists():
        old_reviews = {(r["case_id"], r["repeat"]): r for r in read_csv(output / "run-review.csv")}
    for value in results:
        row = {"case_id": value["caseId"], "repeat": value["repeat"], "status": value["status"],
            "shots": value["shots"], "machine_question": value.get("questionTurnPrelabel"),
            "machine_reply_claims": "|".join(value.get("replyClaimPrelabel", [])),
            "reply": value.get("response", ""), "gold_unnecessary_questions": "", "gold_reply_fact_mismatches": "",
            "gold_genre": "", "gold_genre_correct": "", "gold_enough_qualified_footage": "", "notes": ""}
        old = old_reviews.get((value["caseId"], str(value["repeat"])), {})
        for key in row:
            if key.startswith("gold_") or key == "notes":
                row[key] = old.get(key, "")
        review.append(row)
    if review:
        with (output / "run-review.csv").open("w", encoding="utf-8-sig", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=list(review[0]))
            writer.writeheader()
            writer.writerows(review)
    selections, old_selections = [], {}
    selection_file = output / "selection-review.csv"
    if selection_file.exists():
        old_selections = {(r["case_id"], r["repeat"], r["shot_index"]): r for r in read_csv(selection_file)}
    with sqlite3.connect((snapshot / DB_NAME).as_uri() + "?mode=ro", uri=True) as frozen_db:
        analysis = {a: json.loads(metadata) for a, metadata in frozen_db.execute("SELECT id,metadata_json FROM assets")}
    for value in results:
        evidence = json.loads((output / value["caseId"] / str(value["repeat"]) / "evidence.json").read_text(encoding="utf-8"))
        if not evidence["storyboard_versions"]:
            continue
        board = json.loads(evidence["storyboard_versions"][-1]["content_json"])
        timeline = json.loads(evidence["timeline_versions"][-1]["content_json"]) if evidence["timeline_versions"] else None
        for clip in (timeline.get("clips", []) if timeline else board.get("shots", [])):
            if clip.get("clipKind", "source") != "source":
                continue
            index = clip.get("shotIndex", clip.get("orderIndex"))
            shot = next((s for s in board.get("shots", []) if s.get("orderIndex") == index), {})
            frames = [f.get("imagePath", "") for seg in analysis.get(clip.get("assetId"), {}).get("sceneSegments", []) for f in seg.get("frames", [])]
            row = {"case_id": value["caseId"], "repeat": value["repeat"], "shot_index": index,
                   "asset_id": clip.get("assetId"), "start_ms": clip.get("sourceStartMs"), "end_ms": clip.get("sourceEndMs"),
                   "beat_id": shot.get("beatId"), "purpose": shot.get("purpose"), "machine_match_prelabel": shot.get("matchLevel"),
                   "frames": " | ".join(frames), "gold_hard_risk": "", "gold_direct_support": "", "gold_best_window": "",
                   "gold_relation_violation": "", "notes": ""}
            old = old_selections.get((value["caseId"], str(value["repeat"]), str(index)), {})
            for key in row:
                if key.startswith("gold_") or key == "notes":
                    row[key] = old.get(key, "")
            selections.append(row)
    if selections:
        with selection_file.open("w", encoding="utf-8-sig", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=list(selections[0]))
            writer.writeheader()
            writer.writerows(selections)
    (output / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path)
    parser.add_argument("--source-data", type=Path, default=Path(os.environ["APPDATA"]) / "com.assembly.videoagent")
    parser.add_argument("--snapshot", type=Path)
    parser.add_argument("--project")
    parser.add_argument("--workers", type=int, help="默认全部用例同时发起；只在资源不足时显式降低")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--rescore", action="store_true")
    parser.add_argument("--contract-replay", type=Path, help="只读历史基线，重跑证据适配/P5/计分；不调用模型生成")
    parser.add_argument("--evidence-gold", type=Path, help="原始风险事实金标 CSV，与体裁淘汰金标分开")
    parser.add_argument("--planning-only", action="store_true", help="仅盘点→体裁→配方→有引用策划，不生成成片")
    parser.add_argument("--eligible-evidence", type=Path, help="上游已过底线的 SegmentEvidence JSON；未提供仅做语义隔离评测，不宣称合格")
    parser.add_argument("--gold", type=Path)
    args = parser.parse_args()
    if args.rescore and not args.output:
        parser.error("--rescore 需要 --output 指定已运行目录")
    output = (args.output or ROOT / ".footage-eval" / datetime.datetime.now().strftime("%Y%m%d-%H%M%S")).resolve()
    protected = {args.source_data.resolve(), (Path(os.environ["APPDATA"]) / "com.assembly.videoagent").resolve()}
    if any(output == source or source in output.parents for source in protected):
        raise RuntimeError("评测输出不能位于真实应用数据目录内")
    output.mkdir(parents=True, exist_ok=args.rescore)
    if args.planning_only:
        from planning_only import run_suite
        run_suite(args, output, ROOT, resolve_cases, code_version)
        return
    if args.rescore:
        experiment = json.loads((output / "experiment.json").read_text(encoding="utf-8"))
        snapshot = Path(experiment["snapshot"])
        cases = json.loads((output / "cases.json").read_text(encoding="utf-8"))
        results = []
        for case in cases:
            for repeat in range(1, 4):
                run = output / case["id"] / str(repeat)
                value = score(run, snapshot, case, args.gold, args.evidence_gold)
                prior = json.loads((run / "metrics.json").read_text(encoding="utf-8")) if (run / "metrics.json").exists() else {}
                if "elapsedSeconds" in prior:
                    value["elapsedSeconds"] = prior["elapsedSeconds"]
                if case.get("followup"):
                    add_local_metrics(run, value)
                value.update({"caseId": case["id"], "repeat": repeat})
                write_json(run / "metrics.json", value)
                results.append(value)
        report(output, cases, results, snapshot, experiment["code"])
        write_json(output / "rescore-version.json", {"scoringCode": code_version(), "goldSha256": digest(args.gold) if args.gold else digest(snapshot / "gold.csv")})
        return
    if args.contract_replay:
        if not args.snapshot:
            parser.error("--contract-replay 需要 --snapshot")
        from contract_replay import replay
        if not args.skip_build:
            subprocess.run(["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "footage-eval", "--bin", "footage-eval"], cwd=ROOT, check=True)
        replay(ROOT, args.contract_replay.resolve(), args.snapshot.resolve(), output,
               ROOT / "src-tauri/target/debug/footage-eval.exe", code_version(), args.evidence_gold)
        return
    snapshot = args.snapshot.resolve() if args.snapshot else output / "snapshot"
    if not args.snapshot:
        freeze(args.source_data.resolve(), snapshot, args.project)
    frozen = json.loads((snapshot / "snapshot.json").read_text(encoding="utf-8"))
    if digest(snapshot / DB_NAME) != frozen["databaseSha256"]:
        raise RuntimeError("冻结数据库哈希变化")
    cases = resolve_cases(snapshot)
    version = code_version()
    write_json(output / "cases.json", cases)
    write_json(output / "experiment.json", {"snapshot": str(snapshot), "frozen": frozen, "code": version,
        "casesSha256": digest(output / "cases.json"), "repeats": 3, "track": "frozen_existing_analysis"})
    for item in json.loads((snapshot / "manifest.json").read_text(encoding="utf-8")):
        if item["sha256"] is None or digest(item["source"]) != item["sha256"]:
            raise RuntimeError("原片不存在或内容哈希改变，必须重新冻结")
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "footage-eval", "--bin", "footage-eval"], cwd=ROOT, check=True)
    executable = ROOT / "src-tauri/target/debug/footage-eval.exe"
    dependencies = [*list((snapshot / "runtime-models").rglob("*.onnx")),
                    ROOT / "src-tauri/resources/ffmpeg/ffmpeg.exe", ROOT / "src-tauri/resources/ffmpeg/ffprobe.exe",
                    ROOT / "src-tauri/resources/directml/DirectML.dll"]
    write_json(output / "runtime.json", {"binary": str(executable), "binarySha256": digest(executable),
        "pythonVersion": sys.version, "parallelProcesses": args.workers or len(cases) * 3,
        "dependencies": [{"path": str(p), "sha256": digest(p) if p.exists() else None} for p in dependencies]})
    # ONNX/媒体工具是只读开发资源；不得向主仓库复制/写入构建产物。
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers or len(cases) * 3) as pool:
        pending = {pool.submit(run_one, executable, output / case["id"] / str(repeat), snapshot,
                               frozen["projectId"], case, repeat, args.evidence_gold): (case, repeat) for case in cases for repeat in range(1, 4)}
        for future in concurrent.futures.as_completed(pending):
            results.append(future.result())
            report(output, cases, results, snapshot, version)
    print(f"report: {output / 'report.md'}", flush=True)


if __name__ == "__main__":
    main()

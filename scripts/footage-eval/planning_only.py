"""有引用策划独立真实评测；原快照只读，未供应底线结果时显式标为语义隔离轨道。"""
import concurrent.futures
import json
import statistics
import subprocess
import time
from pathlib import Path

from snapshot import DB_NAME, digest, write_json


def planning_metrics(result, job):
    plan = result.get("result", {})
    proposal = plan.get("proposal")
    sections = proposal.get("sections", []) if proposal else []
    allowed = {e["id"]: e for e in job["eligibleEvidence"]}
    covered, illegal, out_of_window = 0, 0, 0
    reference_count = 0
    for section in sections:
        valid_primary = 0
        for kind in ("claims", "alternatives"):
            for claim in section[kind]:
                reference_count += 1
                ref = claim["reference"]
                evidence = allowed.get(ref["evidenceId"])
                if not evidence or (evidence["assetId"], evidence["segmentId"]) != (ref["assetId"], ref["segmentId"]):
                    illegal += 1
                    continue
                source, window = evidence["range"], ref["range"]
                if not (source["startMs"] <= window["startMs"] < window["endMs"] <= source["endMs"]):
                    out_of_window += 1
                    continue
                if kind == "claims":
                    valid_primary += 1
        covered += int(valid_primary > 0)
    decision = plan.get("genre", {})
    return {"status": plan.get("status", result.get("status")), "sections": len(sections),
            "references": reference_count, "planningReferenceCoveragePct": covered / len(sections) * 100 if sections else None,
            "illegalReferences": illegal if sections else None, "outOfWindowReferences": out_of_window if sections else None,
            "genre": decision.get("genre"), "genreReason": decision.get("reason"),
            "manualSelectionRespected": decision.get("genre") == job["selection"] if job["selection"] != "auto" and decision else None,
            "gaps": plan.get("gaps", []), "rejectionReasons": plan.get("rejectionReasons", []),
            "rejectedAttempts": plan.get("rejectedAttempts", []),
            "error": result.get("error"), "actualDurationMs": plan.get("actualDurationMs"),
            "eligibilityScope": job["eligibilityScope"],
            "mandatoryGapHonestyPrelabel": (plan.get("status") in {"gap_only", "rejected"} and bool(plan.get("gaps"))) if job.get("expectedMissing") else None,
            "semanticCorrectnessGold": None, "genreAccuracyGold": None, "gapHonestyGold": None}


def run_one(executable, root, directory, case, repeat, evidence, eligible, scope):
    (directory / "appdata").mkdir(parents=True, exist_ok=False)
    job = {"mode": "planning-only", "request": case["request"], "selection": case["genre"],
           "durationMs": case.get("targetMs"), "evidence": evidence, "eligibleEvidence": eligible,
           "eligibilityScope": scope, "userFacts": [], "expectedMissing": case.get("expectedMissing", False)}
    write_json(directory / "job.json", job)
    started = time.monotonic()
    with (directory / "process.log").open("w", encoding="utf-8") as log:
        try:
            process = subprocess.run([str(executable), str(directory / "job.json")], cwd=root, stdout=log, stderr=subprocess.STDOUT, timeout=1200)
            exit_code = process.returncode
        except subprocess.TimeoutExpired:
            exit_code = "timeout"
    path = directory / "planning-result.json"
    result = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {"status": "process_failed", "error": f"exit={exit_code}"}
    metrics = planning_metrics(result, job)
    for filename, key in [("genre.json", "decision"), ("inventory.json", "inventory")]:
        p = directory / filename
        if p.exists():
            value = json.loads(p.read_text(encoding="utf-8"))
            if key == "decision":
                metrics["genre"], metrics["genreReason"] = value["genre"], value["reason"]
                metrics["manualSelectionRespected"] = value["genre"] == case["genre"] if case["genre"] != "auto" else None
            else:
                metrics["inventoryCoveredSegments"] = value["coverageCount"]
                metrics["inventoryTotalSegments"] = len(evidence)
                metrics["inventoryGaps"] = value["gaps"]
    metrics.update({"caseId": case["id"], "repeat": repeat, "exitCode": exit_code, "elapsedSeconds": round(time.monotonic() - started, 2)})
    write_json(directory / "metrics.json", metrics)
    return metrics


def report(output, cases, results, scope):
    lines = ["# 任务 4：真实盘点与有引用策划", "", f"底线输入范围：`{scope}`。",
             "未提供上游合格集合时仅评测语义与引用契约；旧风险未知没有变成安全，不能宣称工业合格成片通过。",
             "本模式不合成配音/BGM、不跑局部换镜、选镜、时间线、预览或编辑器。局部换镜用例仅评测其 setup 请求。",
             "人工语义、体裁及缺口金标为空，正确率为 N/A。缺口诚实字段只是机器预标；空结果引用率为 N/A。旧基线策划引用覆盖率 0%。", "",
             "| 用例 | 产策划/尝试 | 每段引用均值/最差 | 非法 ID / 越窗 | 体裁 |", "|---|---:|---|---|---|"]
    summary = {}
    for case in cases:
        runs = [r for r in results if r["caseId"] == case["id"]]
        measured = [r for r in runs if r["sections"]]
        cover = [r["planningReferenceCoveragePct"] for r in measured]
        summary[case["id"]] = {"runs": len(runs), "plans": len(measured), "coverageMean": statistics.mean(cover) if cover else None,
                               "coverageWorst": min(cover) if cover else None, "illegalReferences": sum(r["illegalReferences"] for r in measured) if measured else None,
                               "outOfWindowReferences": sum(r["outOfWindowReferences"] for r in measured) if measured else None,
                               "missingPlanRuns": len(runs) - len(measured), "details": runs}
        value = summary[case["id"]]
        lines.append(f"| {case['id']} | {len(measured)}/{len(runs)} | {value['coverageMean']} / {value['coverageWorst']} | {value['illegalReferences']} / {value['outOfWindowReferences']} | {', '.join(sorted({r['genre'] for r in runs if r.get('genre')}))} |")
    for value in results:
        lines.extend(["", f"## {value['caseId']} / {value['repeat']}", f"状态：{value['status']}；体裁：{value.get('genre')}；理由：{value.get('genreReason')}",
                      f"缺口：{'；'.join(value['gaps'])}", f"拒绝：{'；'.join(value['rejectionReasons'])}", f"被拒提案记录：{value.get('rejectedAttempts', [])}", f"失败：{value.get('error')}"])
    write_json(output / "summary.json", summary)
    (output / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def run_suite(args, output, root, resolve_cases, code_version):
    if not args.snapshot:
        raise ValueError("--planning-only 必须提供 --snapshot；本模式不读取真实应用库")
    if args.workers is not None and not 1 <= args.workers <= 4:
        raise ValueError("本次策划评测 --workers 必须在 1..4")
    snapshot = args.snapshot.resolve()
    frozen = json.loads((snapshot / "snapshot.json").read_text(encoding="utf-8"))
    protected_files = [snapshot / DB_NAME, snapshot / "gold.csv", snapshot / "analysis-original.json", snapshot / "frame-manifest.json"]
    before = {str(p): digest(p) for p in protected_files}
    if before[str(snapshot / DB_NAME)] != frozen["databaseSha256"]:
        raise ValueError("冻结数据库哈希不符")
    # SQLite 导出只在本输出内的副本打开，源快照包括 shm/wal 均不写。
    import shutil
    local_database = output / DB_NAME
    shutil.copy2(snapshot / DB_NAME, local_database)
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "footage-eval", "--bin", "footage-eval"], cwd=root, check=True)
    executable = root / "src-tauri/target/debug/footage-eval.exe"
    subprocess.run([str(executable), "--export-evidence", str(local_database), str(output / "segment-evidence.json")], cwd=root, check=True)
    evidence = json.loads((output / "segment-evidence.json").read_text(encoding="utf-8"))
    manifest = json.loads((snapshot / "manifest.json").read_text(encoding="utf-8"))
    video_ids = {item["assetId"] for item in manifest if item["kind"] == "video"}
    evidence = [item for item in evidence if item["assetId"] in video_ids]
    write_json(output / "segment-evidence.json", evidence)
    for item in manifest:
        if item["kind"] == "video" and (not item["sha256"] or digest(item["source"]) != item["sha256"]):
            raise ValueError("冻结原片不存在或内容已改变")
    if args.eligible_evidence:
        eligible = json.loads(args.eligible_evidence.read_text(encoding="utf-8"))
        base = {(e["assetId"], e["segmentId"], e["analysisSnapshotId"]): e for e in evidence}
        for item in eligible:
            origin = base.get((item["assetId"], item["segmentId"], item["analysisSnapshotId"]))
            if not origin or any(origin.get(key) != item.get(key) for key in
                                 ("schemaVersion", "source", "caption", "visualEvidence", "relations", "motionProfile", "narrativeRole")):
                raise ValueError("合格输入不属于冻结快照")
            if not (origin["range"]["startMs"] <= item["range"]["startMs"] < item["range"]["endMs"] <= origin["range"]["endMs"]):
                raise ValueError("合格输入越出冻结源窗")
        scope = "upstream_eligible_input_assertion_no_gold"
    else:
        eligible = evidence
        scope = "semantic_only_unqualified_frozen_input_not_safety_acceptance"
    cases = resolve_cases(snapshot)
    # 补一例自动叙事缺因果和一例信息不足，核对自动降级与默认。
    cases += [{"id": "auto-narrative-gap", "request": "请剪一个工人遇到故障、维修后恢复生产的因果故事，若缺因果证据可改为真实时刻合集。", "genre": "auto", "targetMs": 20000},
              {"id": "auto-default", "request": "", "genre": "auto"}]
    version = code_version()
    version["recipeVersion"] = "genre-recipe-2026-10-05-v1"
    write_json(output / "cases.json", cases)
    write_json(output / "experiment.json", {"code": version, "snapshot": str(snapshot), "snapshotHashes": before,
               "eligibilityScope": scope, "track": "live_planning_only_frozen_analysis", "repeats": 3,
               "workers": args.workers or 4, "binarySha256": digest(executable), "baselinePlanningCoveragePct": 0})
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers or 4) as pool:
        pending = [pool.submit(run_one, executable, root, output / case["id"] / str(repeat), case, repeat, evidence, eligible, scope)
                   for case in cases for repeat in range(1, 4)]
        for future in concurrent.futures.as_completed(pending):
            value = future.result()
            results.append(value)
            report(output, cases, results, scope)
            print(f"{value['caseId']}/{value['repeat']}: {value['status']} sections={value['sections']} coverage={value['planningReferenceCoveragePct']}", flush=True)
    after = {str(p): digest(p) for p in protected_files}
    write_json(output / "isolation-audit.json", {"before": before, "after": after, "frozenUnchanged": before == after,
               "windowCounts": [json.loads(p.read_text(encoding="utf-8")).get("isolation", {}).get("windows") for p in output.glob("*/*/planning-result.json")]})
    if before != after:
        raise RuntimeError("冻结来源发生变化")
    print(f"report: {output / 'report.md'}", flush=True)

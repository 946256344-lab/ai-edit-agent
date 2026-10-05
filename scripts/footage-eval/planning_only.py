"""有引用策划校准；任务 3 底线门、冻结逐例盘点、独立三次模型策划，来源只读。"""
import concurrent.futures
import json
import statistics
import subprocess
import time
from pathlib import Path

from snapshot import DB_NAME, digest, write_json

SHOT_OPTIONS = {}


def shot_aspect(case):
    aspect = case.get("aspectRatio", case.get("historicalMediaOptions", {}).get("aspectRatio", "16:9"))
    if aspect not in {"16:9", "9:16", "1:1"}:
        raise ValueError("shot evaluation aspectRatio must be explicit and supported")
    return aspect


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
            "rejectionMessages": plan.get("rejectionMessages", []),
            "error": result.get("error"), "actualDurationMs": plan.get("actualDurationMs"),
            "eligibilityScope": job["eligibilityScope"],
            "mandatoryGapHonestyPrelabel": (plan.get("status") in {"gap_only", "rejected"} and bool(plan.get("gaps"))) if job.get("expectedMissing") else None,
            "semanticCorrectnessGold": None, "genreAccuracyGold": None, "gapHonestyGold": None}


def run_one(executable, root, directory, case, repeat, evidence, eligible, scope, frozen_inventory=None, prepare=False, candidate_windows=None):
    (directory / "appdata").mkdir(parents=True, exist_ok=False)
    job = {"mode": "planning-only", "request": case["request"], "selection": case["genre"],
           "durationMs": case.get("targetMs"), "evidence": evidence, "eligibleEvidence": eligible,
           "eligibilityScope": scope, "userFacts": [], "expectedMissing": case.get("expectedMissing", False)}
    if candidate_windows is not None:
        job["candidateWindows"] = candidate_windows
    job["prepareOnly"] = prepare
    if SHOT_OPTIONS:
        import shutil
        shutil.copy2(SHOT_OPTIONS["database"], directory / "appdata" / DB_NAME)
        job["mode"] = "planning-shots"
        job["aspectRatio"] = shot_aspect(case)
        job["projectId"] = SHOT_OPTIONS["projectId"]
        job["verifyCandidates"] = SHOT_OPTIONS["verify"] and not prepare
        job["frozenInventoryForShotEvaluation"] = not job["verifyCandidates"] and not prepare
        if job["verifyCandidates"]:
            job["eligibleEvidence"] = evidence
    if frozen_inventory is not None:
        job["frozenInventory"] = frozen_inventory
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
    actual_eligible = directory / "eligible-evidence.json"
    if actual_eligible.exists():
        job["eligibleEvidence"] = json.loads(actual_eligible.read_text(encoding="utf-8"))
    metrics = planning_metrics(result, job)
    metrics["eligibleSegments"] = len(job["eligibleEvidence"]) if actual_eligible.exists() else None
    metrics["inputCandidates"] = len(eligible)
    metrics["failureCategory"] = result.get("failureCategory")
    metrics["prepareOnly"] = prepare
    if SHOT_OPTIONS and not prepare:
        from shot_metrics import score_shots
        metrics["shotRelations"] = score_shots(directory)
    for filename, key in [("genre.json", "decision"), ("inventory.json", "inventory")]:
        p = directory / filename
        if p.exists():
            value = json.loads(p.read_text(encoding="utf-8"))
            if key == "decision":
                metrics["genre"], metrics["genreReason"] = value["genre"], value["reason"]
                metrics["manualSelectionRespected"] = value["genre"] == case["genre"] if case["genre"] != "auto" else None
            else:
                metrics["inventoryCoveredSegments"] = value["coverageCount"]
                metrics["inventoryTotalSegments"] = len(job["eligibleEvidence"])
                metrics["requirements"] = value.get("requirements", [])
                metrics["footageGapIds"] = sorted(r["id"] for r in value.get("requirements", []) if not r["supports"])
                metrics["inventoryGaps"] = value["gaps"]
    metrics.update({"caseId": case["id"], "repeat": repeat, "exitCode": exit_code, "elapsedSeconds": round(time.monotonic() - started, 2)})
    write_json(directory / "metrics.json", metrics)
    return metrics


def report(output, cases, results, scope, preparations=None):
    preparations = preparations or {}
    lines = ["# 任务 5：策划、组合与证据精修" if SHOT_OPTIONS else "# 任务 4b：策划校准", "", f"底线输入范围：`{scope}`。",
             ("所有输入重过任务 3 底线。每例冻结完整合格盘点，三次独立生成/审核策划和看图选镜，隔离任务 5 效果；体裁每次重新执行。" if SHOT_OPTIONS and not SHOT_OPTIONS["verify"] else "所有策划输入均重新执行任务 3 evaluate；未知不放行。每例冻结底线集合与需求定义；三次均重新盘点、查找支持和生成策划，不复用盘点支持结论。体裁判定每次重新执行，未复用上次判定。"),
             ("组合与精修指标见 shot-report.md。本模式不合成配音/BGM、不跑局部换镜、时间线、预览或编辑器。" if SHOT_OPTIONS else "本模式不合成配音/BGM、不跑局部换镜、选镜、时间线、预览或编辑器。") + "局部换镜用例仅评测其 setup 请求。",
             "人工语义、体裁及缺口金标为空，正确率为 N/A。缺口诚实字段只是机器预标；空结果引用率为 N/A。旧基线策划引用覆盖率 0%。", "",
             "| 用例 | 合格片段 | 准备状态 | 产策划/已开始尝试 | 未开始/3 | 每段引用均值/最差 | 非法 ID / 越窗 | 体裁一致 | 缺口一致 | 拒绝 / 仅缺口 / 服务失败 / 内部失败 |", "|---|---:|---|---:|---:|---|---|---|---|---|"]
    summary = {}
    for case in cases:
        runs = [r for r in results if r["caseId"] == case["id"]]
        measured = [r for r in runs if r["sections"]]
        cover = [r["planningReferenceCoveragePct"] for r in measured]
        summary[case["id"]] = {"runs": len(runs), "plans": len(measured), "coverageMean": statistics.mean(cover) if cover else None,
                               "coverageWorst": min(cover) if cover else None, "illegalReferences": sum(r["illegalReferences"] for r in measured) if measured else None,
                               "outOfWindowReferences": sum(r["outOfWindowReferences"] for r in measured) if measured else None,
                               "missingPlanRuns": len(runs) - len(measured), "details": runs,
                               "eligibleSegments": sorted({r["eligibleSegments"] for r in runs if r["eligibleSegments"] is not None}),
                               "preparation": preparations.get(case["id"]), "notStartedRuns": 3-len(runs),
                               "genreStable": len(runs) == 3 and len({r.get("genre") for r in runs}) == 1 and all(r.get("genre") for r in runs),
                               "footageGapStable": len(runs) == 3 and all("requirements" in r and not r.get("error") for r in runs)
                                   and len({json.dumps(r["footageGapIds"]) for r in runs}) == 1,
                               "statusCounts": {status: sum(r["status"] == status for r in runs) for status in sorted({r["status"] for r in runs})},
                               "externalFailures": sum(r.get("failureCategory") == "external_service" for r in runs),
                               "contractOrRunnerFailures": sum(r.get("failureCategory") == "contract_or_runner" for r in runs)}
        value = summary[case["id"]]
        prep = preparations.get(case["id"], {})
        counts = value["eligibleSegments"] or ([prep["eligibleSegments"]] if prep.get("eligibleSegments") is not None else "N/A")
        statuses = value["statusCounts"]
        lines.append(f"| {case['id']} | {counts} | {prep.get('status', 'N/A')} | {len(measured)}/{len(runs)} | {value['notStartedRuns']} | {value['coverageMean']} / {value['coverageWorst']} | {value['illegalReferences']} / {value['outOfWindowReferences']} | {', '.join(sorted({r['genre'] for r in runs if r.get('genre')}))} / {value['genreStable']} | {value['footageGapStable']} | {statuses.get('rejected', 0)} / {statuses.get('gap_only', 0)} / {value['externalFailures']} / {value['contractOrRunnerFailures']} |")
        if prep.get("error"):
            lines.extend(["", f"准备失败 {case['id']}（{prep.get('failureCategory')}）：{prep['error']}", ""])
    lines.extend(["", "## 汇总", f"已开始 {len(results)}/{len(cases)*3} 次；产策划 {sum(value['plans'] for value in summary.values())} 份；审核拒绝 {sum(value['statusCounts'].get('rejected', 0) for value in summary.values())} 次；仅缺口 {sum(value['statusCounts'].get('gap_only', 0) for value in summary.values())} 次；外部服务失败 {sum(value['externalFailures'] for value in summary.values())} 次；契约/运行器失败 {sum(value['contractOrRunnerFailures'] for value in summary.values())} 次。"])
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
    if args.planning_shots:
        SHOT_OPTIONS.update({"database":local_database,"projectId":frozen["projectId"],"verify":args.verify_candidates})
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "footage-eval", "--bin", "footage-eval"], cwd=root, check=True)
    executable = args.eval_binary.resolve() if args.eval_binary else root / "src-tauri/target/debug/footage-eval.exe"
    subprocess.run([str(executable), "--export-evidence", str(local_database), str(output / "segment-evidence.json")], cwd=root, check=True)
    evidence = json.loads((output / "segment-evidence.json").read_text(encoding="utf-8"))
    manifest = json.loads((snapshot / "manifest.json").read_text(encoding="utf-8"))
    video_ids = {item["assetId"] for item in manifest if item["kind"] == "video"}
    evidence = [item for item in evidence if item["assetId"] in video_ids]
    write_json(output / "segment-evidence.json", evidence)
    for item in manifest:
        if item["kind"] == "video" and (not item["sha256"] or digest(item["source"]) != item["sha256"]):
            raise ValueError("冻结原片不存在或内容已改变")
    candidate_windows = None
    if args.eligible_evidence:
        supplied = json.loads(args.eligible_evidence.read_text(encoding="utf-8"))
        eligible = supplied["evidence"] if isinstance(supplied, dict) else supplied
        candidate_windows = supplied.get("windows") if isinstance(supplied, dict) else None
        if args.planning_shots and isinstance(supplied, dict) and supplied.get("source", {}).get("scope", "").startswith("one_live_verification_run"):
            write_json(output / "verification-audit.json", supplied["source"])
        base = {(e["assetId"], e["segmentId"], e["analysisSnapshotId"]): e for e in evidence}
        for item in eligible:
            origin = base.get((item["assetId"], item["segmentId"], item["analysisSnapshotId"]))
            if not origin or any(origin.get(key) != item.get(key) for key in
                                 ("schemaVersion", "source", "caption", "visualEvidence", "relations", "motionProfile", "narrativeRole")):
                raise ValueError("合格输入不属于冻结快照")
            if not (origin["range"]["startMs"] <= item["range"]["startMs"] < item["range"]["endMs"] <= origin["range"]["endMs"]):
                raise ValueError("合格输入越出冻结源窗")
        scope = "genre_eligibility_evaluate_verified_frozen_input_no_gold"
    else:
        eligible = evidence
        scope = "genre_eligibility_evaluate_original_unknown_input_no_gold"
    cases = resolve_cases(snapshot)
    # 补一例自动叙事缺因果和一例信息不足，核对自动降级与默认。
    cases += [{"id": "auto-narrative-gap", "request": "请剪一个工人遇到故障、维修后恢复生产的因果故事，若缺因果证据可改为真实时刻合集。", "genre": "auto", "targetMs": 20000},
              {"id": "auto-default", "request": "", "genre": "auto"}]
    version = code_version()
    version["recipeVersion"] = "genre-recipe-2026-10-05-v2"
    write_json(output / "cases.json", cases)
    write_json(output / "experiment.json", {"code": version, "snapshot": str(snapshot), "snapshotHashes": before,
               "eligibilityScope": scope, "track": "live_planning_combination_refinement_frozen_analysis" if args.planning_shots else "live_planning_only_frozen_analysis", "repeats": 3,
               "shotRelationsVersion": "shot-relations-v1" if args.planning_shots else None,
               "workers": args.workers or 4, "inventoryTrack": "frozen_inventory_three_live_plans_relations_refinements" if args.planning_shots and not args.verify_candidates else "frozen_requirements_then_three_independent_live_inventories_and_plans", "binarySha256": digest(executable), "baselinePlanningCoveragePct": 0})
    # 单独保存准备阶段。冻结每例的合格集合与需求定义；每次仍独立盘点和对账。
    preparations = {}
    if args.planning_preparation:
        source = args.planning_preparation.resolve()
        for case in cases:
            previous = source / case["id"] / "prepare"
            job = json.loads((previous / "job.json").read_text(encoding="utf-8"))
            if (job["request"] != case["request"] or job["selection"] != case["genre"]
                    or job.get("durationMs") != case.get("targetMs") or job["evidence"] != evidence
                    or job["eligibleEvidence"] != eligible or job.get("candidateWindows") != candidate_windows
                    or job.get("eligibilityScope") != scope
                    or (args.planning_shots and job.get("aspectRatio", "16:9") != shot_aspect(case))):
                raise ValueError(f"{case['id']}: 冻结准备阶段与本轮输入不一致")
            value = json.loads((previous / "metrics.json").read_text(encoding="utf-8"))
            if value["status"] != "prepared":
                raise ValueError(f"{case['id']}: 不能复用未完成的准备阶段")
            directory = output / case["id"] / "prepare"
            directory.mkdir(parents=True)
            hashes = {}
            for name in ("inventory.json", "eligible-evidence.json", "genre.json", "eligibility.json"):
                shutil.copy2(previous / name, directory / name)
                hashes[name] = digest(directory / name)
            # 再次中断后仍可核对完整输入并续跑，不能只留下支持结果却丢掉 job/metrics。
            shutil.copy2(previous / "job.json", directory / "job.json")
            value.update({"frozenFrom": str(previous), "frozenArtifactHashes": hashes})
            write_json(directory / "metrics.json", value)
            preparations[case["id"]] = value
    else:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers or 4) as pool:
            pending = {pool.submit(run_one, executable, root, output / case["id"] / "prepare", case, 0,
                                  evidence, eligible, scope, None, True, candidate_windows): case for case in cases}
            for future in concurrent.futures.as_completed(pending):
                case = pending[future]
                value = future.result()
                preparations[case["id"]] = value
                print(f"prepare {case['id']}: {value['status']} eligible={value['eligibleSegments']}", flush=True)
    write_json(output / "preparation-summary.json", preparations)
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers or 4) as pool:
        pending = []
        for case in cases:
            directory = output / case["id"] / "prepare"
            if preparations[case["id"]]["status"] != "prepared":
                # 未开始的运行不伪装成三次模型失败。
                print(f"{case['id']}: preparation failed, three plan attempts not started", flush=True)
                continue
            frozen_inventory = json.loads((directory / "inventory.json").read_text(encoding="utf-8"))
            frozen_eligible = json.loads((directory / "eligible-evidence.json").read_text(encoding="utf-8"))
            for repeat in range(1, 4):
                pending.append(pool.submit(run_one, executable, root, output / case["id"] / str(repeat),
                                           case, repeat, evidence, frozen_eligible, scope, frozen_inventory))
        for future in concurrent.futures.as_completed(pending):
            value = future.result()
            results.append(value)
            report(output, cases, results, scope, preparations)
            print(f"{value['caseId']}/{value['repeat']}: {value['status']} sections={value['sections']} coverage={value['planningReferenceCoveragePct']}", flush=True)
    report(output, cases, results, scope, preparations)
    if args.planning_shots:
        from shot_metrics import report_shots
        report_shots(output, results, snapshot.parent / "baseline-final-2026-10-01")
    after = {str(p): digest(p) for p in protected_files}
    write_json(output / "isolation-audit.json", {"before": before, "after": after, "frozenUnchanged": before == after,
               "windowCounts": [json.loads(p.read_text(encoding="utf-8")).get("isolation", {}).get("windows") for p in output.glob("*/*/planning-result.json")]})
    if before != after:
        raise RuntimeError("冻结来源发生变化")
    print(f"report: {output / 'report.md'}", flush=True)

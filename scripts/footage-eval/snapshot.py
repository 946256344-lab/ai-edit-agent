"""冻结只读应用快照、94 条原片哈希、分析/向量及历史需求；只写输出根。"""
import csv
import hashlib
import json
import shutil
import sqlite3
from pathlib import Path

DB_NAME = "assembly-video-agent.sqlite3"


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def write_json(path, value):
    def binary(value):
        if isinstance(value, bytes):
            return {"sha256": hashlib.sha256(value).hexdigest(), "byteLength": len(value), "storage": DB_NAME}
        raise TypeError(type(value).__name__)
    Path(path).write_text(json.dumps(value, ensure_ascii=False, indent=2, default=binary), encoding="utf-8")


def connect(path):
    connection = sqlite3.connect(path)
    connection.row_factory = sqlite3.Row
    return connection


def rows(connection, query, args=()):
    return [dict(row) for row in connection.execute(query, args)]


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for child in value.values():
            yield from strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from strings(child)


def remap(value, mapping):
    if isinstance(value, str):
        return mapping.get(value, value)
    if isinstance(value, dict):
        return {key: remap(child, mapping) for key, child in value.items()}
    if isinstance(value, list):
        return [remap(child, mapping) for child in value]
    return value


def freeze(source, root, project=None, database_snapshot=None):
    root.mkdir(parents=True, exist_ok=True)
    if (root / "snapshot.json").exists():
        raise RuntimeError("冻结快照已完成；请传 --snapshot 重用或选择新输出目录")
    # SQLite online backup 包含已提交 WAL；源连接只读，不调用迁移/恢复/分析。
    if not (root / DB_NAME).exists():
        original = sqlite3.connect((database_snapshot or source / DB_NAME).as_uri() + "?mode=ro", uri=True)
        target = sqlite3.connect(root / DB_NAME)
        original.backup(target)
        original.close()
        target.close()
    c = connect(root / DB_NAME)
    if project is None:
        found = rows(c, "SELECT project_id FROM assets WHERE kind='video' GROUP BY project_id HAVING COUNT(*)=94")
        if len(found) != 1:
            raise RuntimeError("需用 --project 指定唯一的 94 条工业素材项目")
        project = found[0]["project_id"]
    original_assets = rows(c, "SELECT * FROM assets WHERE project_id=? ORDER BY id", (project,))
    original_videos = [a for a in original_assets if a["kind"] == "video"]
    if len(original_videos) != 94:
        raise RuntimeError(f"期望 94 条视频，实际 {len(original_videos)}")
    # 项目原有 ID 已被移除并重新导入；按同一源路径绑定当前活跃分析，不从名字猜内容。
    videos, bindings = [], []
    for old in original_videos:
        active = rows(c, "SELECT * FROM assets WHERE source_reference=? AND kind='video' AND COALESCE(json_extract(metadata_json,'$.libraryRemoved'),0)=0 ORDER BY updated_at DESC,id", (old["source_reference"],))
        chosen = active[0] if active else old
        videos.append(chosen)
        bindings.append({"source": old["source_reference"], "historicalAssetId": old["id"], "frozenAssetId": chosen["id"],
                         "activeAnalysisFound": bool(active), "activeDuplicates": len(active)})
    assets = sorted(videos + [a for a in original_assets if a["kind"] != "video"], key=lambda a: a["id"])
    write_json(root / "inventory-bindings.json", bindings)
    write_json(root / "analysis-original.json", assets)
    identifiers = [a["id"] for a in assets]
    embeddings = rows(c, "SELECT * FROM asset_segment_embeddings WHERE asset_id IN (" + ",".join("?" for _ in identifiers) + ")", identifiers)
    write_json(root / "vectors.json", embeddings)
    history = rows(c, "SELECT * FROM agent_tasks WHERE project_id=? ORDER BY created_at", (project,))
    messages = rows(c, "SELECT m.* FROM messages m JOIN conversations c ON c.id=m.conversation_id WHERE c.project_id=? ORDER BY m.created_at", (project,))
    write_json(root / "history.json", {"tasks": history, "messages": messages})
    boards = rows(c, "SELECT * FROM storyboard_versions WHERE project_id=? ORDER BY created_at", (project,))
    write_json(root / "historical-storyboards.json", boards)
    usage = {}
    for board in boards:
        for shot in json.loads(board["content_json"]).get("shots", []):
            aid = shot.get("assetId")
            if aid:
                usage[aid] = usage.get(aid, 0) + 1
    write_json(root / "historical-usage.json", usage)
    manifest, mapping, prelabels = [], {}, []
    for index, asset in enumerate(assets, 1):
        print(f"freeze {index}/{len(assets)}", flush=True)
        metadata = json.loads(asset["metadata_json"])
        source_path = Path(asset["source_reference"])
        source_hash = digest(source_path) if source_path.is_file() else None
        manifest.append({"assetId": asset["id"], "kind": asset["kind"], "source": str(source_path),
                         "bytes": source_path.stat().st_size if source_path.is_file() else None,
                         "sha256": source_hash, "analysisSha256": hashlib.sha256(asset["metadata_json"].encode()).hexdigest(),
                         "analysisStatus": asset["analysis_status"], "historicalUsageCount": usage.get(asset["id"], 0),
                         "evalInitialUsageCount": 0})
        for value in strings(metadata):
            path = Path(value)
            if not path.is_absolute() or source not in path.parents or not path.is_file():
                continue
            if value not in mapping:
                copied = root / "analysis-files" / path.relative_to(source)
                copied.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, copied)
                mapping[value] = str(copied)
        for segment in metadata.get("sceneSegments", []):
            evidence = segment.get("visualEvidence") or {}
            detail = evidence.get("detail") or {}
            # 仅匹配模型已写的画面描述/风险字段，不从文件名猜测，不将未知标成安全。
            evidence_text = json.dumps(evidence, ensure_ascii=False)
            risk_text = json.dumps([evidence.get("qualityNotes", []), evidence.get("caption", ""), evidence.get("scene", ""),
                                   detail.get("focus", ""), evidence.get("cameraMotion", "")], ensure_ascii=False).lower()
            risk = []
            risk_words = {
                "品牌标识": ["logo", "brand", "标识", "商标"],
                "展会/展厅": ["exhibition", "trade show", "booth", "showroom", "展会", "展厅", "展台"],
                "失焦": ["out of focus", "defocus", "blurry", "blurred", "失焦", "模糊"],
                "抖动": ["shaky", "shake", "unstable", "抖动"],
                "杂乱背景": ["clutter", "杂乱"],
            }
            for name, words in risk_words.items():
                if any(word in risk_text for word in words):
                    risk.append(name)
            if detail.get("brandLogos") and "品牌标识" not in risk:
                risk.append("品牌标识")
            if detail.get("exhibition") is True and "展会/展厅" not in risk:
                risk.append("展会/展厅")
            frames = segment.get("frames", [])
            prelabels.append({"asset_id": asset["id"], "segment_id": segment.get("id"),
                "start_ms": segment.get("startMs"), "end_ms": segment.get("endMs"),
                "machine_risks": "|".join(risk) or "未知/无正向风险词", "machine_status": "预标，待用户纠正",
                "evidence": evidence_text, "frames": " | ".join(mapping.get(f.get("imagePath"), f.get("imagePath", "")) for f in frames),
                "machine_best_range": json.dumps(detail.get("bestRange"), ensure_ascii=False),
                "machine_concepts": "|".join(detail.get("concepts", [])), "machine_genre_hint": "工业画面，可能宣传；体裁待纠正",
                "confirmed": "", "genre": "", "gold_risks": "", "risk_start_ms": "", "risk_end_ms": "",
                "brand_identity": "", "best_start_ms": "", "best_end_ms": "", "supports": "", "split": "", "notes": ""})
        rewritten = remap(metadata, mapping)
        c.execute("UPDATE assets SET project_id=?, metadata_json=? WHERE id=?", (project, json.dumps(rewritten, ensure_ascii=False), asset["id"]))
    # 只保留冻结库，历史结果已先独立导出。评测中不重新激活旧的 removed 记录。
    c.execute("DELETE FROM assets WHERE id NOT IN (" + ",".join("?" for _ in identifiers) + ")", identifiers)
    c.execute("DELETE FROM asset_segment_embeddings WHERE asset_id NOT IN (SELECT id FROM assets)")
    c.commit()
    write_json(root / "manifest.json", manifest)
    write_json(root / "frame-manifest.json", [{"path": path, "sha256": digest(path)} for path in mapping.values()])
    with (root / "gold.csv").open("w", encoding="utf-8-sig", newline="") as stream:
        if prelabels:
            writer = csv.DictWriter(stream, fieldnames=list(prelabels[0]))
            writer.writeheader()
            writer.writerows(prelabels)
    # 只复制选镜的权重，不触碰真实缓存。每个运行可从此输出内的副本建硬链接。
    runtime = source / "runtime-models"
    if runtime.exists():
        shutil.copytree(runtime, root / "runtime-models")
    c.close()
    write_json(root / "snapshot.json", {"projectId": project, "videoCount": len(videos),
        "databaseSha256": digest(root / DB_NAME), "analysisSha256": digest(root / "analysis-original.json"),
        "vectorsSha256": digest(root / "vectors.json"), "manifestSha256": digest(root / "manifest.json"),
        "historicalUsage": str(root / "historical-usage.json"), "evalInitialUsage": 0,
        "mode": "frozen_existing_analysis", "reimportAnalysis": "not_run_separate_track"})
    return project

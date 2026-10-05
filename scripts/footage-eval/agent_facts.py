"""任务 7 隔离复跑：沿用基线，仅选相关用例、补 genre 快照与问答/切点回归。"""
import argparse
import json
import sys
from pathlib import Path

import run


def install_cases():
    original_resolve, original_add, original_score = run.resolve_cases, run.add_task, run.score

    def resolve(snapshot):
        cases = [c for c in original_resolve(snapshot) if c["id"] in
                 {"history-1", "history-2", "history-3", "local-replace", "en-15-voice"}]
        local = next(c for c in cases if c["id"] == "local-replace")
        cases.append({**local, "id": "local-refine", "followup":
                      "只微调第一个镜头的入点和出点，去掉开始和结束的无效画面。保持素材身份、该镜头的时间线时长、其余镜头和声音不变。",
                      "provenance": "任务 7 切点回归，非历史基线"})
        for locale, request in [("zh-CN", "解释一下宣传片、叙事片和花絮片有什么区别？不要生成或修改视频。"),
                                ("en", "How do I plan a promotional video? Explain the workflow; do not create or change a video.")]:
            cases.append({"id": "answer-" + locale, "request": request, "targetMs": 30000,
                          "voiceover": True, "bgm": True, "genre": "promotion", "uiLocale": locale,
                          "answerOnly": True, "provenance": "任务 7 普通问答回归，非生成请求"})
        cases.append({**cases[-1], "id": "answer-link", "request":
                      "Reply with the link https://example.com and one sentence explaining that it is an example link. Do not create or change a video.",
                      "provenance": "任务 7 HTTPS 网址不可误判为内部路径的回归"})
        return cases

    def add(directory, project, case, phase, storyboard=None, timeline=None):
        original_add(directory, project, case, phase, storyboard, timeline)
        jobpath = directory / "job.json"
        job = json.loads(jobpath.read_text(encoding="utf-8"))
        job["mediaOptions"]["genre"] = case["genre"]
        run.write_json(jobpath, job)
        connection = run.connect(directory / "appdata" / run.DB_NAME)
        connection.execute("UPDATE agent_tasks SET input_json=json_set(input_json,'$.mediaOptions',json(?)) WHERE id=?",
                           (json.dumps(job["mediaOptions"]), job["agentTaskId"]))
        connection.commit()
        connection.close()

    def score(directory, snapshot, case, *args, **kwargs):
        metrics = original_score(directory, snapshot, case, *args, **kwargs)
        if case.get("answerOnly"):
            # 普通问答没有生成目标，不能套用生成请求的“完成无产物”分母。
            metrics.update({"completedWithoutArtifact": None, "goalReceiptMismatch": None,
                            "answerOnly": True})
        return metrics

    run.resolve_cases, run.add_task, run.score = resolve, add, score


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--answer-smoke", action="store_true")
    args, _ = parser.parse_known_args()
    if not 1 <= args.workers <= 4:
        parser.error("任务 7 workers 必须在 1 到 4 之间")
    if "--workers" not in sys.argv:
        sys.argv.extend(["--workers", str(args.workers)])
    install_cases()
    if args.answer_smoke:
        sys.argv.remove("--answer-smoke")
        resolve_all = run.resolve_cases

        def resolve_answers(snapshot):
            return [case for case in resolve_all(snapshot) if case.get("answerOnly")]

        run.resolve_cases = resolve_answers
    run.main()


if __name__ == "__main__":
    main()

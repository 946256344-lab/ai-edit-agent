import json
import msvcrt
import os
import subprocess
import sys
import time
import uuid
from contextlib import contextmanager
from pathlib import Path

from pyJianYingDraft import DraftFolder, Timerange, TrackType, VideoMaterial, VideoSegment


SOURCE_DURATION_TOLERANCE_US = 50_000


def to_microseconds(milliseconds):
    return milliseconds * 1000


def fit_source_duration(source_start_us, requested_duration_us, material_duration_us):
    if requested_duration_us <= 0:
        raise RuntimeError("Timeline clip duration must be positive.")
    available_duration_us = material_duration_us - source_start_us
    if available_duration_us <= 0:
        raise RuntimeError("Timeline clip starts at or beyond the source media duration.")
    if requested_duration_us <= available_duration_us:
        return requested_duration_us
    overflow_us = requested_duration_us - available_duration_us
    if overflow_us > SOURCE_DURATION_TOLERANCE_US:
        raise RuntimeError(
            f"Timeline clip exceeds the source media duration by {overflow_us / 1000:.0f} ms."
        )
    return available_duration_us


def create_cover(source, source_start_ms, destination):
    subprocess.run(
        [
            "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
            "-ss", f"{source_start_ms / 1000:.3f}", "-i", str(source),
            "-frames:v", "1", str(destination),
        ],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def same_path(left, right):
    return os.path.normcase(os.path.normpath(str(left))) == os.path.normcase(os.path.normpath(str(right)))


def jianying_is_running():
    result = subprocess.run(
        ["tasklist", "/FI", "IMAGENAME eq JianyingPro.exe", "/FO", "CSV", "/NH"],
        check=False,
        capture_output=True,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    return b"jianyingpro.exe" in (result.stdout or b"").lower()


@contextmanager
def registry_write_lock(registry_path):
    lock_path = registry_path.with_name(f".{registry_path.name}.assembly-video-agent.lock")
    with lock_path.open("a+b") as lock_file:
        lock_file.seek(0, os.SEEK_END)
        if lock_file.tell() == 0:
            lock_file.write(b"\0")
            lock_file.flush()
        deadline = time.monotonic() + 10
        while True:
            try:
                lock_file.seek(0)
                msvcrt.locking(lock_file.fileno(), msvcrt.LK_NBLCK, 1)
                break
            except OSError:
                if time.monotonic() >= deadline:
                    raise RuntimeError("Another Assembly Video Agent Jianying export is still running.")
                time.sleep(0.1)
        try:
            yield
        finally:
            lock_file.seek(0)
            msvcrt.locking(lock_file.fileno(), msvcrt.LK_UNLCK, 1)


def register_draft(registry_path, root, draft_path, draft_name, duration_ms):
    with registry_write_lock(registry_path):
        for _ in range(3):
            snapshot = registry_path.read_bytes()
            registry = json.loads(snapshot.decode("utf-8"))
            drafts = registry.get("all_draft_store")
            if not isinstance(drafts, list):
                raise RuntimeError("Jianying draft registry is invalid.")
            if any(
                same_path(draft.get("draft_fold_path", ""), draft_path)
                for draft in drafts
            ):
                return
            template = next(
                (draft for draft in drafts if same_path(draft.get("draft_root_path", ""), root)),
                None,
            )
            if template is None:
                raise RuntimeError("Jianying draft library is not registered.")

            entry = dict(template)
            now = time.time_ns() // 1000
            cover = draft_path / "draft_cover.jpg"
            entry.update({
                "cloud_draft_cover": False,
                "cloud_draft_sync": False,
                "draft_cover": str(cover) if cover.is_file() else "",
                "draft_fold_path": str(draft_path),
                "draft_id": str(uuid.uuid4()).upper(),
                "draft_is_invisible": False,
                "draft_json_file": str(draft_path / "draft_content.json"),
                "draft_name": draft_name,
                "draft_root_path": template["draft_root_path"],
                "draft_timeline_materials_size": sum(
                    source.stat().st_size for source in draft_path.rglob("*") if source.is_file()
                ),
                "tm_draft_create": now,
                "tm_draft_modified": now,
                "tm_duration": to_microseconds(duration_ms),
            })
            drafts.insert(0, entry)
            temporary_path = registry_path.with_name(
                f".{registry_path.name}.{uuid.uuid4().hex}.tmp"
            )
            try:
                temporary_path.write_text(
                    json.dumps(registry, ensure_ascii=False, separators=(",", ":")),
                    encoding="utf-8",
                )
                if registry_path.read_bytes() != snapshot:
                    time.sleep(0.05)
                    continue
                os.replace(temporary_path, registry_path)
                return
            finally:
                temporary_path.unlink(missing_ok=True)
        raise RuntimeError(
            "Jianying draft registry changed during export. Close Jianying Pro and try again."
        )


def register_when_safe(registry_path, root, draft_path, draft_name, duration_ms):
    if jianying_is_running():
        return "pending"
    register_draft(registry_path, root, draft_path, draft_name, duration_ms)
    return "registered"


def registration_result(draft_path, registration_status):
    return {
        "draftDirectory": str(draft_path),
        "draftContentPath": str(draft_path / "draft_content.json"),
        "registrationStatus": registration_status,
    }


def main():
    if len(sys.argv) != 2:
        raise RuntimeError("Jianying draft adapter requires a versioned input file.")
    payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    if payload.get("inputFormatVersion") != 1:
        raise RuntimeError("Jianying draft adapter received an unsupported input format.")
    operation = payload.get("operation", "createDraft")
    root = Path(payload["draftRoot"])
    draft_name = payload["draftName"]
    registry_path = Path(payload["draftRegistryPath"])
    if not root.is_dir():
        raise RuntimeError("Jianying draft root is unavailable.")
    if operation == "registerDraft":
        draft_path = Path(payload["draftDirectory"])
        if not (draft_path / "draft_content.json").is_file():
            raise RuntimeError("Pending Jianying draft content is unavailable.")
        if jianying_is_running():
            raise RuntimeError("Jianying Pro is still running; registration remains pending.")
        register_draft(
            registry_path,
            root,
            draft_path,
            draft_name,
            payload["durationMs"],
        )
        print(json.dumps(registration_result(draft_path, "registered")))
        return
    if operation != "createDraft":
        raise RuntimeError("Jianying draft adapter received an unsupported operation.")
    if (root / draft_name).exists():
        raise RuntimeError("Jianying draft already exists; it was not overwritten.")
    prepared_clips = []
    duration_ms = 0
    for index, clip in enumerate(payload["clips"], start=1):
        source = Path(clip["sourceReference"])
        if not source.is_file():
            raise RuntimeError(f"Timeline source media file {index} is unavailable.")
        timeline_duration_us = to_microseconds(
            clip["timelineEndMs"] - clip["timelineStartMs"]
        )
        source_start_us = to_microseconds(clip["sourceStartMs"])
        material = VideoMaterial(str(source))
        source_duration_us = fit_source_duration(
            source_start_us,
            timeline_duration_us,
            material.duration,
        )
        prepared_clips.append(
            (clip, material, timeline_duration_us, source_start_us, source_duration_us)
        )
        duration_ms = max(duration_ms, clip["timelineEndMs"])

    script = DraftFolder(str(root)).create_draft(draft_name, 540, 960, 30, allow_replace=False)
    script.add_track(TrackType.video)
    for clip, material, timeline_duration_us, source_start_us, source_duration_us in prepared_clips:
        segment = VideoSegment(
            material,
            Timerange(to_microseconds(clip["timelineStartMs"]), timeline_duration_us),
            source_timerange=Timerange(source_start_us, source_duration_us),
        )
        script.add_segment(segment)
    script.save()
    if payload["clips"]:
        first_clip = payload["clips"][0]
        create_cover(Path(first_clip["sourceReference"]), first_clip["sourceStartMs"], root / draft_name / "draft_cover.jpg")
    draft_path = root / draft_name
    registration_status = register_when_safe(
        registry_path,
        root,
        draft_path,
        draft_name,
        duration_ms,
    )
    print(json.dumps(registration_result(draft_path, registration_status)))


if __name__ == "__main__":
    main()

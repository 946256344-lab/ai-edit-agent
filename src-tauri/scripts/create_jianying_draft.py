import json
import msvcrt
import os
import shutil
import subprocess
import sys
import time
import uuid
from contextlib import contextmanager
from pathlib import Path

from pyJianYingDraft import (
    AudioMaterial,
    AudioSegment,
    ClipSettings,
    DraftFolder,
    FontType,
    TextBackground,
    TextBorder,
    TextIntro,
    TextOutro,
    TextSegment,
    TextShadow,
    TextStyle,
    Timerange,
    TrackType,
    VideoMaterial,
    VideoSegment,
)


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


def new_draft_path(root, draft_name):
    if not isinstance(draft_name, str) or not draft_name or Path(draft_name).name != draft_name:
        raise RuntimeError("Jianying draft name must be one local directory name.")
    resolved_root = root.resolve(strict=True)
    draft_path = (resolved_root / draft_name).resolve(strict=False)
    if draft_path.parent != resolved_root:
        raise RuntimeError("Jianying draft path is outside the selected draft root.")
    return draft_path


def text_color(value):
    value = str(value or "#FFFFFF").lstrip("#")
    if len(value) != 6:
        return (1.0, 1.0, 1.0)
    try:
        return tuple(int(value[index:index + 2], 16) / 255 for index in (0, 2, 4))
    except ValueError:
        return (1.0, 1.0, 1.0)


def text_alignment(value):
    return {"left": 0, "center": 1, "right": 2}.get(value, 1)


JIAN_YING_FONT_KEYS = {
    "jianying_sans_bold": "SourceHanSansCN_Bold",
    "jianying_sans_regular": "SourceHanSansCN_Regular",
    "jianying_serif_bold": "SourceHanSerifCN_Bold",
    "jianying_handwritten": "LXGWWenKai_Bold",
    "jianying_harmony_bold": "HarmonyOS_Sans_SC_Bold",
}


def jianying_font(font_key):
    enum_name = JIAN_YING_FONT_KEYS.get(font_key)
    return getattr(FontType, enum_name, None) if enum_name else None


def jianying_text_border(style_data):
    width = float(style_data.get("strokeWidth", 0))
    if width <= 0:
        return None
    return TextBorder(
        color=text_color(style_data.get("strokeColor", "#000000")),
        width=min(100.0, width * 10.0),
    )


def jianying_text_background(style_data):
    color = style_data.get("backgroundColor")
    if not color:
        return None
    return TextBackground(color=str(color), round_radius=0.16, height=0.14, width=0.14)


def jianying_text_shadow(style_data):
    if not style_data.get("shadow", False):
        return None
    return TextShadow()


def add_supported_text_animation(segment, animation, phase):
    if not animation:
        return
    template_id = animation.get("templateId")
    duration = max(0, int(animation.get("durationMs", 0))) * 1000
    if template_id in ("fade", "wipe"):
        segment.add_animation(TextIntro.渐显 if phase == "in" else TextOutro.渐隐, duration)
    elif template_id == "slide_up" and phase == "in":
        segment.add_animation(TextIntro.向上滑动, duration)
    elif template_id == "slide_down" and phase == "in":
        segment.add_animation(TextIntro.向下滑动, duration)
    elif template_id == "pop" and phase == "in":
        segment.add_animation(TextIntro.弹入, duration)


def escape_text_material_unicode(script):
    """Keep nested text JSON ASCII-escaped for current Jianying readers."""
    for material in script.materials.texts:
        content = material.get("content")
        if isinstance(content, str):
            material["content"] = json.dumps(
                json.loads(content), ensure_ascii=True, separators=(",", ":")
            )


def add_text_tracks(script, tracks):
    enabled_tracks = [track for track in tracks if track.get("enabled", True)]
    if not enabled_tracks:
        return
    ordered_tracks = sorted(
        enumerate(enabled_tracks),
        key=lambda item: (int(item[1].get("layer", 0)), item[0]),
    )
    for track_index, track in ordered_tracks:
        layer = int(track.get("layer", 0))
        track_name = f"assembly-text-layer-{layer}-{track_index}"
        script.add_track(TrackType.text, track_name=track_name, relative_index=layer)
        for cue in track.get("cues", []):
            duration_ms = int(cue["endMs"]) - int(cue["startMs"])
            if duration_ms <= 0:
                raise RuntimeError("Text cue duration must be positive.")
            style_data = cue.get("style", {})
            layout = cue.get("layout", {})
            style = TextStyle(
                size=max(1.0, float(style_data.get("fontSize", 0.08)) * 100),
                bold=bool(style_data.get("bold", False)),
                color=text_color(style_data.get("color")),
                align=text_alignment(style_data.get("alignment")),
                letter_spacing=int(style_data.get("letterSpacing", 0)),
                line_spacing=int(style_data.get("lineSpacing", 0)),
                auto_wrapping=True,
                max_line_width=float(layout.get("maxWidth", 0.82)),
            )
            segment = TextSegment(
                cue["text"],
                Timerange(to_microseconds(int(cue["startMs"])), to_microseconds(duration_ms)),
                font=jianying_font(style_data.get("fontKey")),
                style=style,
                clip_settings=ClipSettings(
                    transform_x=(float(layout.get("x", 0.5)) - 0.5) * 2,
                    transform_y=(0.5 - float(layout.get("y", 0.5))) * 2,
                ),
                border=jianying_text_border(style_data),
                background=jianying_text_background(style_data),
                shadow=jianying_text_shadow(style_data),
            )
            add_supported_text_animation(segment, cue.get("entrance"), "in")
            add_supported_text_animation(segment, cue.get("exit"), "out")
            script.add_segment(segment, track_name=track_name)
            escape_text_material_unicode(script)


def add_music_tracks(script, tracks):
    enabled_tracks = [track for track in tracks if track.get("enabled", True)]
    for track_index, track in enumerate(enabled_tracks):
        track_name = f"assembly-music-{track_index}"
        script.add_track(TrackType.audio, track_name=track_name)
        for cue in track.get("cues", []):
            source = Path(cue["sourceReference"])
            if not source.is_file():
                raise RuntimeError("Music source media is unavailable.")
            source_start_us = to_microseconds(int(cue["sourceStartMs"]))
            source_duration_us = to_microseconds(
                int(cue["sourceEndMs"]) - int(cue["sourceStartMs"])
            )
            timeline_start_us = to_microseconds(int(cue["timelineStartMs"]))
            timeline_end_us = to_microseconds(int(cue["timelineEndMs"]))
            material = AudioMaterial(str(source))
            remaining_us = timeline_end_us - timeline_start_us
            segment_start_us = timeline_start_us
            while remaining_us > 0:
                segment_duration_us = min(source_duration_us, remaining_us)
                if segment_duration_us < remaining_us and not cue.get("loopEnabled", False):
                    raise RuntimeError("Non-looping music does not cover its timeline range.")
                segment = AudioSegment(
                    material,
                    Timerange(segment_start_us, segment_duration_us),
                    source_timerange=Timerange(source_start_us, segment_duration_us),
                    volume=float(cue.get("volume", 1.0)),
                )
                is_first = segment_start_us == timeline_start_us
                is_last = segment_duration_us == remaining_us
                fade_in_us = to_microseconds(int(cue.get("fadeInMs", 0))) if is_first else 0
                fade_out_us = to_microseconds(int(cue.get("fadeOutMs", 0))) if is_last else 0
                if fade_in_us or fade_out_us:
                    segment.add_fade(fade_in_us, fade_out_us)
                script.add_segment(segment, track_name=track_name)
                segment_start_us += segment_duration_us
                remaining_us -= segment_duration_us


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
    payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8-sig"))
    if payload.get("inputFormatVersion") not in (1, 2):
        raise RuntimeError("Jianying draft adapter received an unsupported input format.")
    operation = payload.get("operation", "createDraft")
    root = Path(payload["draftRoot"])
    draft_name = payload["draftName"]
    registry_path = Path(payload["draftRegistryPath"])
    if not root.is_dir():
        raise RuntimeError("Jianying draft root is unavailable.")
    draft_path = new_draft_path(root, draft_name)
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
    if draft_path.exists():
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

    try:
        script = DraftFolder(str(root)).create_draft(
            draft_name, 540, 960, 30, allow_replace=False
        )
        script.add_track(TrackType.video)
        for clip, material, timeline_duration_us, source_start_us, source_duration_us in prepared_clips:
            segment = VideoSegment(
                material,
                Timerange(to_microseconds(clip["timelineStartMs"]), timeline_duration_us),
                source_timerange=Timerange(source_start_us, source_duration_us),
            )
            script.add_segment(segment)
        add_text_tracks(script, payload.get("textTracks", []))
        add_music_tracks(script, payload.get("musicTracks", []))
        script.save()
        if payload["clips"]:
            first_clip = payload["clips"][0]
            create_cover(
                Path(first_clip["sourceReference"]),
                first_clip["sourceStartMs"],
                draft_path / "draft_cover.jpg",
            )
        registration_status = register_when_safe(
            registry_path,
            root,
            draft_path,
            draft_name,
            duration_ms,
        )
    except BaseException:
        shutil.rmtree(draft_path, ignore_errors=True)
        raise
    print(json.dumps(registration_result(draft_path, registration_status)))


if __name__ == "__main__":
    main()

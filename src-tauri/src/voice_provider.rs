//! ElevenLabs 配音领域：指纹缓存、alignment 字幕与时间线写入。
//! 密钥和 HTTP 由 `music_provider` 拥有；本模块不访问 Credential Manager 或网络。

use crate::assets::{store_downloaded_audio, wait_for_asset_ready};
use crate::models::{TextAnimation, TextCue, TextLayout, TextStyle, TextTrack, TimelineVersion};
use crate::music_provider::elevenlabs_json_request;
use crate::process::{hidden_command, run_hidden_command_with_timeout, HiddenCommandError};
use crate::timeline_voice::apply_synthesized_voiceover;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const DEFAULT_VOICE_ID: &str = "IKne3meq5aSn9XLyUdCD";
const DEFAULT_MODEL_ID: &str = "eleven_multilingual_v2";
const OUTPUT_FORMAT: &str = "mp3_44100_128";
const PRODUCT_CHAR_LIMIT: usize = 5_000;
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_AUDIO_BYTES: u64 = 16 * 1024 * 1024;

const DEFAULT_VOICE_SETTINGS: &str =
    r#"{"stability":0.45,"similarity_boost":0.8,"style":0.2,"use_speaker_boost":true,"speed":1.0}"#;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSummary {
    pub voice_id: String,
    pub name: String,
    pub category: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceoverApplyResult {
    pub asset_id: String,
    pub generation_id: String,
    pub duration_ms: i64,
    pub timeline_version_id: String,
    pub subtitle_cue_count: usize,
    pub reused_cache: bool,
    pub quality_warnings: Vec<crate::models::PreviewQualityCheck>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceoverManifest {
    fingerprint: String,
    voice_id: String,
    voice_name: String,
    model_id: String,
    duration_ms: i64,
    status: String,
}

#[derive(Debug)]
pub(crate) struct CachedVoiceover {
    generation_id: String,
    directory: PathBuf,
    manifest: VoiceoverManifest,
}

pub(crate) trait VoiceTransport {
    fn get_json(&self, path: &str) -> Result<Value, String>;
    fn post_json(&self, path: &str, body: Value) -> Result<Value, String>;
}

struct ElevenLabsTransport;

impl VoiceTransport for ElevenLabsTransport {
    fn get_json(&self, path: &str) -> Result<Value, String> {
        elevenlabs_json_request("GET", path, None)
    }

    fn post_json(&self, path: &str, body: Value) -> Result<Value, String> {
        elevenlabs_json_request("POST", path, Some(body))
    }
}

fn provider_char_limit(model_id: &str) -> usize {
    match model_id {
        "eleven_multilingual_v2" => 10_000,
        "eleven_v3" => 5_000,
        "eleven_flash_v2_5" => 40_000,
        _ => 5_000,
    }
}

pub(crate) fn normalize_narration_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn generation_fingerprint(
    text: &str,
    voice_id: &str,
    model_id: &str,
    voice_settings: &str,
    output_format: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize_narration_text(text).as_bytes());
    hasher.update(b"|");
    hasher.update(voice_id.as_bytes());
    hasher.update(b"|");
    hasher.update(model_id.as_bytes());
    hasher.update(b"|");
    hasher.update(voice_settings.as_bytes());
    hasher.update(b"|");
    hasher.update(output_format.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn tts_request_body(text: &str) -> Value {
    json!({
        "text": text,
        "model_id": DEFAULT_MODEL_ID,
    })
}

fn validate_narration_text(text: &str, model_id: &str) -> Result<String, String> {
    let normalized = normalize_narration_text(text);
    if normalized.is_empty() {
        return Err("Narration text is required; on-screen titles are not spoken.".to_owned());
    }
    let chars = normalized.chars().count();
    if chars > PRODUCT_CHAR_LIMIT {
        return Err("Narration exceeds the 5000 character product limit.".to_owned());
    }
    if chars > provider_char_limit(model_id) {
        return Err("Narration exceeds the selected ElevenLabs model character limit.".to_owned());
    }
    Ok(normalized)
}

fn voices_from_payload(payload: &Value) -> Vec<VoiceSummary> {
    payload
        .get("voices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let voice_id = row.get("voice_id")?.as_str()?.trim();
            if voice_id.is_empty() {
                return None;
            }
            Some(VoiceSummary {
                voice_id: voice_id.to_owned(),
                name: row
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Unnamed")
                    .to_owned(),
                category: row
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            })
        })
        .collect()
}

pub(crate) fn resolve_voice_id(
    requested: Option<&str>,
    voices: &[VoiceSummary],
) -> Result<(String, String), String> {
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());
    if let Some(voice_id) = requested {
        let match_row = voices.iter().find(|voice| voice.voice_id == voice_id);
        return match match_row {
            Some(voice) => Ok((voice.voice_id.clone(), voice.name.clone())),
            None => Err(unavailable_voice_message(voices)),
        };
    }
    voices
        .iter()
        .find(|voice| voice.voice_id == DEFAULT_VOICE_ID)
        .map(|voice| (voice.voice_id.clone(), voice.name.clone()))
        .ok_or_else(|| unavailable_voice_message(voices))
}

fn unavailable_voice_message(voices: &[VoiceSummary]) -> String {
    let names = voices
        .iter()
        .take(8)
        .map(|voice| voice.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if names.is_empty() {
        "Charlie is unavailable and no ElevenLabs voices were returned.".to_owned()
    } else {
        format!("Charlie is unavailable. Choose one of: {names}.")
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct AlignmentCue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

pub(crate) fn cues_from_alignment(
    alignment: &Value,
    voice_duration_ms: i64,
) -> Result<Vec<AlignmentCue>, String> {
    let characters = alignment
        .get("characters")
        .and_then(Value::as_array)
        .ok_or_else(|| "incomplete_alignment".to_owned())?;
    let starts = alignment
        .get("character_start_times_seconds")
        .and_then(Value::as_array)
        .ok_or_else(|| "incomplete_alignment".to_owned())?;
    let ends = alignment
        .get("character_end_times_seconds")
        .and_then(Value::as_array)
        .ok_or_else(|| "incomplete_alignment".to_owned())?;
    if characters.is_empty() || characters.len() != starts.len() || characters.len() != ends.len() {
        return Err("incomplete_alignment".to_owned());
    }
    let mut letters = String::new();
    let mut start_secs = Vec::with_capacity(characters.len());
    let mut end_secs = Vec::with_capacity(characters.len());
    for index in 0..characters.len() {
        let Some(ch) = characters[index].as_str() else {
            continue;
        };
        let Some(start) = starts[index].as_f64() else {
            return Err("incomplete_alignment".to_owned());
        };
        let Some(end) = ends[index].as_f64() else {
            return Err("incomplete_alignment".to_owned());
        };
        letters.push_str(ch);
        start_secs.push(start);
        end_secs.push(end);
    }
    if letters.trim().is_empty() {
        return Err("incomplete_alignment".to_owned());
    }
    let words: Vec<(usize, usize)> = letters
        .char_indices()
        .filter(|(_, ch)| !ch.is_whitespace())
        .fold(Vec::new(), |mut words, (byte_index, ch)| {
            if words.last().is_some_and(|(_, end)| *end == byte_index) {
                if let Some(last) = words.last_mut() {
                    last.1 = byte_index + ch.len_utf8();
                }
            } else {
                words.push((byte_index, byte_index + ch.len_utf8()));
            }
            words
        });
    if words.is_empty() {
        return Err("incomplete_alignment".to_owned());
    }
    let char_index_at_byte = |byte_index: usize| letters[..byte_index].chars().count();
    let mut cues = Vec::new();
    let mut cursor = 0usize;
    while cursor < words.len() {
        let mut end_word = (cursor + 10).min(words.len());
        for candidate in cursor..end_word {
            let word = &letters[words[candidate].0..words[candidate].1];
            if candidate - cursor >= 3
                && word.chars().last().is_some_and(|ch| {
                    matches!(ch, '.' | '!' | '?' | '。' | '！' | '？' | ',' | '，')
                })
            {
                end_word = candidate + 1;
                break;
            }
        }
        if end_word < words.len() && words.len() - end_word == 1 {
            end_word = words.len();
        }
        let first = words[cursor];
        let last = words[end_word - 1];
        let text = letters[first.0..last.1].trim().to_owned();
        if text.is_empty() {
            cursor = end_word;
            continue;
        }
        let start_index = char_index_at_byte(first.0);
        let last_char_index = char_index_at_byte(last.1).saturating_sub(1);
        let start_ms = (start_secs[start_index] * 1000.0).floor() as i64;
        let end_ms = (end_secs[last_char_index] * 1000.0).ceil() as i64;
        let end_ms = end_ms.max(start_ms + 1).min(voice_duration_ms);
        if end_ms > start_ms {
            cues.push(AlignmentCue {
                start_ms: start_ms.max(0),
                end_ms,
                text,
            });
        }
        cursor = end_word;
    }
    if cues.is_empty() {
        return Err("incomplete_alignment".to_owned());
    }
    if cues
        .last()
        .is_some_and(|cue| cue.end_ms > voice_duration_ms)
    {
        return Err("incomplete_alignment".to_owned());
    }
    Ok(cues)
}

fn subtitle_track_from_cues(generation_id: &str, cues: &[AlignmentCue]) -> TextTrack {
    TextTrack {
        id: format!("voice-alignment-{generation_id}"),
        role: "subtitle".to_owned(),
        layer: 1,
        enabled: true,
        origin: "voice_alignment".to_owned(),
        generation_id: Some(generation_id.to_owned()),
        editable: true,
        locked: false,
        cues: cues
            .iter()
            .enumerate()
            .map(|(index, cue)| TextCue {
                id: format!("voice-alignment-{generation_id}-{index}"),
                template_id: Some("subtitle_safe".to_owned()),
                start_ms: cue.start_ms,
                end_ms: cue.end_ms,
                text: cue.text.chars().take(280).collect(),
                style: TextStyle::default(),
                layout: TextLayout::default(),
                entrance: Some(TextAnimation {
                    template_id: "fade".to_owned(),
                    duration_ms: 180,
                    intensity: 0.6,
                }),
                exit: Some(TextAnimation {
                    template_id: "fade".to_owned(),
                    duration_ms: 160,
                    intensity: 0.5,
                }),
                loop_animation: None,
                jianying_compatibility: "verified".to_owned(),
            })
            .collect(),
    }
}

fn project_lock(project_id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let map = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .entry(project_id.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn voiceover_root(app: &AppHandle, project_id: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("voiceovers")
        .join(project_id);
    fs::create_dir_all(&directory)
        .map_err(|_| "Could not prepare local voiceover storage.".to_owned())?;
    Ok(directory)
}

fn read_cached_generation(root: &Path, fingerprint: &str) -> Option<CachedVoiceover> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let directory = entry.path();
        if !directory.is_dir() {
            continue;
        }
        let manifest_path = directory.join("manifest.json");
        let audio_path = directory.join("voiceover.mp3");
        let Ok(raw) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<VoiceoverManifest>(&raw) else {
            continue;
        };
        if manifest.status == "success"
            && manifest.fingerprint == fingerprint
            && audio_path.is_file()
        {
            return Some(CachedVoiceover {
                generation_id: directory
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                directory,
                manifest,
            });
        }
    }
    None
}

pub(crate) fn probe_audio_duration_ms(path: &Path) -> Result<i64, String> {
    let mut command = hidden_command("ffprobe");
    command.args([
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "json",
    ]);
    command.arg(path);
    let output =
        run_hidden_command_with_timeout(&mut command, PROBE_TIMEOUT).map_err(
            |error| match error {
                HiddenCommandError::TimedOut => {
                    "FFprobe timed out while reading the voiceover.".to_owned()
                }
                HiddenCommandError::Failed => {
                    "FFprobe is not available on this computer.".to_owned()
                }
            },
        )?;
    if !output.status.success() {
        return Err("FFprobe could not read the voiceover.".to_owned());
    }
    let payload: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "FFprobe returned invalid voiceover metadata.".to_owned())?;
    let seconds = payload
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or_else(|| "Voiceover has no verified duration.".to_owned())?;
    if seconds <= 0.0 {
        return Err("Voiceover has no verified duration.".to_owned());
    }
    Ok((seconds * 1000.0).ceil() as i64)
}

fn write_generation_files(
    directory: &Path,
    audio: &[u8],
    alignment: &Value,
    manifest: &VoiceoverManifest,
) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|_| "Could not prepare voiceover generation storage.".to_owned())?;
    fs::write(directory.join("voiceover.mp3"), audio)
        .map_err(|_| "Could not save the voiceover audio.".to_owned())?;
    fs::write(
        directory.join("alignment.json"),
        serde_json::to_vec(alignment).map_err(|error| error.to_string())?,
    )
    .map_err(|_| "Could not save voiceover alignment.".to_owned())?;
    fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec(manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|_| "Could not save the voiceover manifest.".to_owned())?;
    Ok(())
}

fn decode_audio_payload(payload: &Value) -> Result<Vec<u8>, String> {
    let encoded = payload
        .get("audio_base64")
        .and_then(Value::as_str)
        .ok_or_else(|| "ElevenLabs did not return audio.".to_owned())?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "ElevenLabs returned invalid audio encoding.".to_owned())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_AUDIO_BYTES {
        return Err("ElevenLabs returned an unusable audio payload.".to_owned());
    }
    Ok(bytes)
}

fn alignment_payload(payload: &Value) -> Result<&Value, String> {
    payload
        .get("normalized_alignment")
        .or_else(|| payload.get("alignment"))
        .filter(|value| value.is_object())
        .ok_or_else(|| "incomplete_alignment".to_owned())
}

pub(crate) fn synthesize_with_transport(
    transport: &dyn VoiceTransport,
    text: &str,
    requested_voice_id: Option<&str>,
    root: &Path,
) -> Result<(CachedVoiceover, Vec<u8>, Value, bool), String> {
    let text = validate_narration_text(text, DEFAULT_MODEL_ID)?;
    let voices = voices_from_payload(&transport.get_json("/voices")?);
    let (voice_id, voice_name) = resolve_voice_id(requested_voice_id, &voices)?;
    let fingerprint = generation_fingerprint(
        &text,
        &voice_id,
        DEFAULT_MODEL_ID,
        DEFAULT_VOICE_SETTINGS,
        OUTPUT_FORMAT,
    );
    if let Some(cached) = read_cached_generation(root, &fingerprint) {
        let audio = fs::read(cached.directory.join("voiceover.mp3"))
            .map_err(|_| "Cached voiceover audio is no longer available.".to_owned())?;
        let alignment = fs::read_to_string(cached.directory.join("alignment.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or_else(|| json!({}));
        return Ok((cached, audio, alignment, true));
    }
    let path = format!(
        "/text-to-speech/{}/with-timestamps?output_format={OUTPUT_FORMAT}",
        urlencoding_minimal(&voice_id)
    );
    let payload = transport.post_json(&path, tts_request_body(&text))?;
    let audio = decode_audio_payload(&payload)?;
    let alignment = alignment_payload(&payload)?.clone();
    let generation_id = Uuid::new_v4().to_string();
    let directory = root.join(&generation_id);
    let staging = root.join(format!("staging-{generation_id}"));
    let manifest = VoiceoverManifest {
        fingerprint,
        voice_id: voice_id.clone(),
        voice_name: voice_name.clone(),
        model_id: DEFAULT_MODEL_ID.to_owned(),
        duration_ms: 0,
        status: "success".to_owned(),
    };
    write_generation_files(&staging, &audio, &alignment, &manifest)?;
    fs::rename(&staging, &directory)
        .map_err(|_| "Could not commit the voiceover generation.".to_owned())?;
    Ok((
        CachedVoiceover {
            generation_id,
            directory,
            manifest: VoiceoverManifest {
                voice_id,
                voice_name,
                ..manifest
            },
        },
        audio,
        alignment,
        false,
    ))
}

fn urlencoding_minimal(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(crate) fn list_voices_for_agent() -> Result<Vec<VoiceSummary>, String> {
    Ok(voices_from_payload(
        &ElevenLabsTransport.get_json("/voices")?,
    ))
}

pub(crate) fn resolve_tool_narration_text(
    tool_text: Option<&str>,
    storyboard_narration: Option<&str>,
) -> Result<String, String> {
    if let Some(text) = tool_text.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(text.to_owned());
    }
    if let Some(text) = storyboard_narration
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(text.to_owned());
    }
    Err("Narration text is required; on-screen titles are not spoken.".to_owned())
}

pub(crate) fn storyboard_narration_text(
    storyboard: Option<&crate::models::StoryboardVersion>,
) -> Option<String> {
    let storyboard = storyboard?;
    let joined = storyboard
        .shots
        .iter()
        .map(|shot| shot.narration_text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!joined.is_empty()).then_some(joined)
}

pub(crate) fn synthesize_voiceover_for_timeline(
    app: &AppHandle,
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    timeline: &TimelineVersion,
    text: &str,
    voice_id: Option<&str>,
) -> Result<(TimelineVersion, VoiceoverApplyResult), String> {
    let lock = project_lock(project_id);
    let _guard = lock
        .lock()
        .map_err(|_| "Voiceover generation is already running for this project.".to_owned())?;
    let root = voiceover_root(app, project_id)?;
    let (cached, _audio, alignment, reused) =
        synthesize_with_transport(&ElevenLabsTransport, text, voice_id, &root)?;
    let mp3_path = cached.directory.join("voiceover.mp3");
    let duration_ms = probe_audio_duration_ms(&mp3_path)?;
    let subtitle_track = match cues_from_alignment(&alignment, duration_ms) {
        Ok(cues) => Some(subtitle_track_from_cues(&cached.generation_id, &cues)),
        Err(_) if reused => {
            return Err(
                "Cached voiceover alignment is incomplete; subtitles were not committed."
                    .to_owned(),
            )
        }
        Err(_) => return Err(
            "Voiceover audio was stored but alignment is incomplete; subtitles were not committed."
                .to_owned(),
        ),
    };
    let Some(subtitle_track) = subtitle_track else {
        return Err("Voiceover alignment is incomplete; subtitles were not committed.".to_owned());
    };
    let display_name = format!("ElevenLabs: {} voiceover", cached.manifest.voice_name);
    let asset = store_downloaded_audio(app, project_id, mp3_path.clone(), &display_name)?;
    let asset = wait_for_asset_ready(app, project_id, &asset.id)?;
    let cue_count = subtitle_track.cues.len();
    let (version, warnings) = apply_synthesized_voiceover(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        timeline,
        &asset.id,
        &cached.generation_id,
        &cached.manifest.voice_id,
        &cached.manifest.voice_name,
        duration_ms,
        Some(subtitle_track),
    )?;
    let mut manifest = cached.manifest;
    manifest.duration_ms = duration_ms;
    let _ = fs::write(
        cached.directory.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap_or_default(),
    );
    Ok((
        version.clone(),
        VoiceoverApplyResult {
            asset_id: asset.id,
            generation_id: cached.generation_id,
            duration_ms,
            timeline_version_id: version.id,
            subtitle_cue_count: cue_count,
            reused_cache: reused,
            quality_warnings: warnings,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    struct SequenceTransport {
        voices: Value,
        payloads: Vec<Value>,
        posts: Cell<usize>,
    }

    impl VoiceTransport for SequenceTransport {
        fn get_json(&self, path: &str) -> Result<Value, String> {
            if path == "/voices" {
                return Ok(self.voices.clone());
            }
            Err("unexpected GET".to_owned())
        }

        fn post_json(&self, _path: &str, _body: Value) -> Result<Value, String> {
            let index = self.posts.get();
            self.posts.set(index + 1);
            self.payloads
                .get(index)
                .cloned()
                .ok_or_else(|| "ElevenLabs request timed out.".to_owned())
        }
    }

    fn charlie_voices() -> Value {
        json!({"voices":[{"voice_id": DEFAULT_VOICE_ID, "name": "Charlie", "category": "premade"}]})
    }

    fn alignment_for(text: &str) -> Value {
        let characters: Vec<Value> = text.chars().map(|ch| json!(ch.to_string())).collect();
        let starts: Vec<Value> = text
            .chars()
            .enumerate()
            .map(|(index, _)| json!(index as f64 * 0.12))
            .collect();
        let ends: Vec<Value> = text
            .chars()
            .enumerate()
            .map(|(index, _)| json!(index as f64 * 0.12 + 0.1))
            .collect();
        json!({
            "characters": characters,
            "character_start_times_seconds": starts,
            "character_end_times_seconds": ends
        })
    }

    fn tts_payload(text: &str) -> Value {
        json!({
            "audio_base64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"ID3fake"),
            "normalized_alignment": alignment_for(text)
        })
    }

    #[test]
    fn fingerprint_changes_when_text_or_voice_changes() {
        let first = generation_fingerprint(
            "Hello world",
            "v1",
            DEFAULT_MODEL_ID,
            DEFAULT_VOICE_SETTINGS,
            OUTPUT_FORMAT,
        );
        let spaced = generation_fingerprint(
            "Hello   world",
            "v1",
            DEFAULT_MODEL_ID,
            DEFAULT_VOICE_SETTINGS,
            OUTPUT_FORMAT,
        );
        let other_text = generation_fingerprint(
            "Hello worlds",
            "v1",
            DEFAULT_MODEL_ID,
            DEFAULT_VOICE_SETTINGS,
            OUTPUT_FORMAT,
        );
        let other_voice = generation_fingerprint(
            "Hello world",
            "v2",
            DEFAULT_MODEL_ID,
            DEFAULT_VOICE_SETTINGS,
            OUTPUT_FORMAT,
        );
        assert_eq!(first, spaced);
        assert_ne!(first, other_text);
        assert_ne!(first, other_voice);
    }

    #[test]
    fn empty_or_oversize_text_is_rejected() {
        assert!(validate_narration_text("   ", DEFAULT_MODEL_ID).is_err());
        let huge: String = "a".repeat(PRODUCT_CHAR_LIMIT + 1);
        assert!(validate_narration_text(&huge, DEFAULT_MODEL_ID).is_err());
    }

    #[test]
    fn missing_charlie_does_not_fall_back_to_another_voice() {
        let voices = vec![VoiceSummary {
            voice_id: "other".to_owned(),
            name: "Adam".to_owned(),
            category: "premade".to_owned(),
        }];
        let error = resolve_voice_id(None, &voices).unwrap_err();
        assert!(error.contains("Adam"));
        assert!(resolve_voice_id(Some("other"), &voices).is_ok());
    }

    #[test]
    fn alignment_builds_cues_and_keeps_punctuation() {
        let text = "Hello, world. Next line!";
        let cues = cues_from_alignment(&alignment_for(text), 10_000).expect("cues");
        assert!(!cues.is_empty());
        assert!(cues.iter().all(|cue| cue.end_ms <= 10_000));
        assert!(cues[0].text.contains("Hello"));
    }

    #[test]
    fn incomplete_alignment_fails_closed() {
        let payload = json!({"characters":["a"], "character_start_times_seconds":[0.0]});
        assert_eq!(
            cues_from_alignment(&payload, 1000).unwrap_err(),
            "incomplete_alignment"
        );
    }

    #[test]
    fn identical_synthesis_reuses_cache_without_a_second_post() {
        let directory = std::env::temp_dir().join(format!("voice-cache-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("temp");
        let transport = SequenceTransport {
            voices: charlie_voices(),
            payloads: vec![tts_payload("Cached line.")],
            posts: Cell::new(0),
        };
        let first = synthesize_with_transport(&transport, "Cached line.", None, &directory)
            .expect("first synthesis");
        let second = synthesize_with_transport(&transport, "Cached line.", None, &directory)
            .expect("cached synthesis");
        assert!(!first.3);
        assert!(second.3);
        assert_eq!(transport.posts.get(), 1);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn timeout_is_not_retried_by_transport() {
        let directory = std::env::temp_dir().join(format!("voice-timeout-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("temp");
        let transport = SequenceTransport {
            voices: charlie_voices(),
            payloads: Vec::new(),
            posts: Cell::new(0),
        };
        let error =
            synthesize_with_transport(&transport, "Timeout line.", None, &directory).unwrap_err();
        assert!(error.contains("timed out"));
        assert_eq!(transport.posts.get(), 1);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn tts_request_body_is_the_documented_minimal_payload() {
        let body = tts_request_body("They check how materials are managed.");
        assert_eq!(body["text"], "They check how materials are managed.");
        assert_eq!(body["model_id"], DEFAULT_MODEL_ID);
        assert!(body.get("voice_settings").is_none());
    }

    #[test]
    fn tool_text_is_preferred_over_storyboard_narration() {
        let resolved =
            resolve_tool_narration_text(Some("Spoken copy"), Some("Shot narration")).expect("text");
        assert_eq!(resolved, "Spoken copy");
        assert!(resolve_tool_narration_text(None, None).is_err());
    }
}

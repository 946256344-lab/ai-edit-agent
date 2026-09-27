//! 项目品牌套件与默认转场：存 projects.settings_json 的 brandKit / defaultTransition。
//! logo、字体按内容哈希复制进应用数据目录，从不覆盖或删除；旧时间线快照引用的文件因此仍可重建卡片。
use crate::db::{now_millis, open_connection};
use crate::models::{BrandSnapshot, TransitionSpec};
use base64::Engine;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub(crate) const DEFAULT_PRIMARY: &str = "#FFFFFF";
pub(crate) const DEFAULT_ACCENT: &str = "#D9E2EC";
const LOGO_LIMIT_BYTES: u64 = 5 * 1024 * 1024;
const FONT_LIMIT_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrandKit {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub cta: String,
    #[serde(default)]
    pub primary_color: String,
    #[serde(default)]
    pub accent_color: String,
    #[serde(default)]
    pub logo_file: Option<String>,
    #[serde(default)]
    pub font_file: Option<String>,
}

impl BrandKit {
    /// 设置了名称或 logo 才算有品牌套件；生成时据此决定是否自动加开场 / 片尾 / 角标。
    pub(crate) fn is_set(&self) -> bool {
        !self.name.trim().is_empty() || self.logo_file.is_some()
    }

    pub(crate) fn snapshot(&self) -> BrandSnapshot {
        let color = |value: &str, fallback: &str| {
            if is_color(value) { value.to_uppercase() } else { fallback.to_owned() }
        };
        BrandSnapshot {
            name: self.name.trim().to_owned(),
            handle: self.handle.trim().to_owned(),
            primary_color: color(&self.primary_color, DEFAULT_PRIMARY),
            accent_color: color(&self.accent_color, DEFAULT_ACCENT),
            logo_file: self.logo_file.clone(),
            font_file: self.font_file.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandKitView {
    #[serde(flatten)]
    pub kit: BrandKit,
    /// logo 以 data URL 给界面预览，不暴露本机路径。
    pub logo_preview: Option<String>,
    pub font_name: Option<String>,
    pub default_transition: TransitionSpec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandKitInput {
    pub name: String,
    pub handle: String,
    pub cta: String,
    pub primary_color: String,
    pub accent_color: String,
    #[serde(default)]
    pub logo_source_path: Option<String>,
    #[serde(default)]
    pub clear_logo: bool,
    #[serde(default)]
    pub font_source_path: Option<String>,
    #[serde(default)]
    pub clear_font: bool,
    pub default_transition: TransitionSpec,
}

pub(crate) fn is_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|character| character.is_ascii_hexdigit())
}

pub(crate) fn brand_dir(app: &AppHandle, project_id: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("brand")
        .join(project_id);
    std::fs::create_dir_all(&directory).map_err(|_| "Could not prepare the brand kit folder.".to_owned())?;
    Ok(directory)
}

fn read_settings(connection: &Connection, project_id: &str) -> Result<serde_json::Value, String> {
    let raw: String = connection
        .query_row(
            "SELECT settings_json FROM projects WHERE id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|_| "Project was not found.".to_owned())?;
    Ok(serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({})))
}

pub(crate) fn read_brand_kit(connection: &Connection, project_id: &str) -> BrandKit {
    read_settings(connection, project_id)
        .ok()
        .and_then(|settings| serde_json::from_value(settings.get("brandKit")?.clone()).ok())
        .unwrap_or_default()
}

/// 项目默认转场；没设置时为硬切。
pub(crate) fn read_default_transition(connection: &Connection, project_id: &str) -> TransitionSpec {
    read_settings(connection, project_id)
        .ok()
        .and_then(|settings| serde_json::from_value(settings.get("defaultTransition")?.clone()).ok())
        .and_then(|spec| crate::timeline_graphics::normalize_transition_spec(&spec).ok())
        .unwrap_or_else(|| TransitionSpec { kind: "none".to_owned(), duration_ms: crate::timeline_graphics::DEFAULT_TRANSITION_MS })
}

fn mime_for(file: &str) -> &'static str {
    match Path::new(file).extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// 复制进品牌目录，文件名带内容哈希；同内容重复导入复用同一文件。
fn import_file(
    directory: &Path,
    source: &str,
    prefix: &str,
    allowed: &[&str],
    limit: u64,
    kind_label: &str,
) -> Result<String, String> {
    let source = Path::new(source);
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|ext| allowed.contains(&ext.as_str()))
        .ok_or_else(|| format!("{kind_label} must be one of: {}.", allowed.join(", ")))?;
    let metadata = std::fs::metadata(source).map_err(|_| format!("{kind_label} file is unavailable."))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(format!("{kind_label} file must be under {} MB.", limit / 1024 / 1024));
    }
    let bytes = std::fs::read(source).map_err(|_| format!("{kind_label} file could not be read."))?;
    let digest = Sha256::digest(&bytes);
    let hash = digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect::<String>();
    let name = format!("{prefix}-{hash}.{extension}");
    let destination = directory.join(&name);
    if !destination.is_file() {
        std::fs::write(&destination, &bytes).map_err(|_| format!("{kind_label} could not be saved."))?;
    }
    Ok(name)
}

fn view(app: &AppHandle, connection: &Connection, project_id: &str) -> Result<BrandKitView, String> {
    let kit = read_brand_kit(connection, project_id);
    let directory = brand_dir(app, project_id)?;
    let logo_preview = kit.logo_file.as_deref().and_then(|file| {
        let bytes = std::fs::read(directory.join(file)).ok()?;
        Some(format!(
            "data:{};base64,{}",
            mime_for(file),
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    });
    let font_name = kit
        .font_file
        .as_deref()
        .and_then(|file| Path::new(file).extension()?.to_str())
        .map(|extension| extension.to_uppercase());
    Ok(BrandKitView {
        font_name,
        logo_preview,
        default_transition: read_default_transition(connection, project_id),
        kit,
    })
}

#[tauri::command(async)]
pub fn get_brand_kit(app: AppHandle, project_id: String) -> Result<BrandKitView, String> {
    let connection = open_connection(&app)?;
    view(&app, &connection, &project_id)
}

#[tauri::command(async)]
pub fn set_brand_kit(
    app: AppHandle,
    project_id: String,
    input: BrandKitInput,
) -> Result<BrandKitView, String> {
    let connection = open_connection(&app)?;
    let mut settings = read_settings(&connection, &project_id)?;
    let previous = read_brand_kit(&connection, &project_id);
    let text = |value: &str, limit: usize, label: &str| -> Result<String, String> {
        let trimmed = value.trim();
        if trimmed.chars().any(char::is_control) || trimmed.chars().count() > limit {
            return Err(format!("{label} must be at most {limit} characters on one line."));
        }
        Ok(trimmed.to_owned())
    };
    let color = |value: &str| -> Result<String, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() || is_color(trimmed) {
            Ok(trimmed.to_uppercase())
        } else {
            Err("Brand colors must look like #RRGGBB.".to_owned())
        }
    };
    let directory = brand_dir(&app, &project_id)?;
    let logo_file = match (&input.logo_source_path, input.clear_logo) {
        (Some(path), _) => Some(import_file(&directory, path, "logo", &["png", "jpg", "jpeg", "webp", "svg"], LOGO_LIMIT_BYTES, "Logo")?),
        (None, true) => None,
        (None, false) => previous.logo_file.clone(),
    };
    let font_file = match (&input.font_source_path, input.clear_font) {
        (Some(path), _) => Some(import_file(&directory, path, "font", &["ttf", "otf", "woff", "woff2"], FONT_LIMIT_BYTES, "Font")?),
        (None, true) => None,
        (None, false) => previous.font_file.clone(),
    };
    let kit = BrandKit {
        name: text(&input.name, 40, "Brand name")?,
        handle: text(&input.handle, 48, "Handle or website")?,
        cta: text(&input.cta, 40, "Call to action")?,
        primary_color: color(&input.primary_color)?,
        accent_color: color(&input.accent_color)?,
        logo_file,
        font_file,
    };
    let transition = crate::timeline_graphics::normalize_transition_spec(&input.default_transition)?;
    settings["brandKit"] = serde_json::to_value(&kit).map_err(|error| error.to_string())?;
    settings["defaultTransition"] = serde_json::to_value(&transition).map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE projects SET settings_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![settings.to_string(), now_millis(), project_id],
        )
        .map_err(|error| error.to_string())?;
    view(&app, &connection, &project_id)
}

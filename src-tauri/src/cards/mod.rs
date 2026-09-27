//! 品牌卡：HTML/CSS 模板 + 本地 WebView2 渲染成透明 PNG，供预览叠加和编辑器图片交付。
//! PNG 是按「模板、文案、品牌快照、画幅」派生的缓存，丢了就重渲染；交付时复制到交付物旁边。
pub(crate) mod templates;
#[cfg(windows)]
mod renderer;

use crate::media_options::Canvas;
use crate::models::GraphicOverlay;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// 卡片按预览画布的 2 倍渲染（竖屏 1080×1920），交给编辑器时足够清晰，预览再缩小叠加。
pub(crate) fn card_size(canvas: Canvas) -> (u32, u32) {
    ((canvas.width * 2) as u32, (canvas.height * 2) as u32)
}

fn aspect_name(canvas: Canvas) -> &'static str {
    match canvas.width.cmp(&canvas.height) {
        std::cmp::Ordering::Less => "portrait",
        std::cmp::Ordering::Greater => "landscape",
        std::cmp::Ordering::Equal => "square",
    }
}

fn brand_url(file: Option<&str>, brand_dir: &Path) -> Option<String> {
    let file = file?;
    brand_dir
        .join(file)
        .is_file()
        .then(|| format!("https://{BRAND_HOST}/{file}"))
}

const BRAND_HOST: &str = "brand.voycut.local";

/// 返回这张卡当前画幅下的 PNG；缓存命中直接返回，否则本地渲染。
pub(crate) fn ensure_card_png(
    app: &AppHandle,
    project_id: &str,
    overlay: &GraphicOverlay,
    canvas: Canvas,
) -> Result<PathBuf, String> {
    let template = templates::card_template(&overlay.template_id)
        .ok_or_else(|| format!("Card template {} is not available.", overlay.template_id))?;
    let (width, height) = card_size(canvas);
    let key = crate::preview_cache::key(&(
        template.fingerprint(),
        &overlay.template_id,
        &overlay.slots,
        &overlay.brand,
        width,
        height,
    ));
    let directory = crate::preview_cache::project_cache_dir(app, project_id)?.join("cards");
    std::fs::create_dir_all(&directory).map_err(|_| "Could not prepare the card cache.".to_owned())?;
    let destination = directory.join(format!("card-{key}.png"));
    if destination.is_file() {
        return Ok(destination);
    }
    let brand_dir = crate::brand_kit::brand_dir(app, project_id)?;
    let logo_url = brand_url(overlay.brand.logo_file.as_deref(), &brand_dir);
    if template.manifest.requires_logo && logo_url.is_none() {
        return Err("The brand logo for this card is missing; set it again in Project settings.".to_owned());
    }
    let card = serde_json::json!({
        "aspect": aspect_name(canvas),
        "primaryColor": overlay.brand.primary_color,
        "accentColor": overlay.brand.accent_color,
        "name": overlay.brand.name,
        "handle": overlay.brand.handle,
        "slots": overlay.slots,
        "logoUrl": logo_url,
        "fontUrl": brand_url(overlay.brand.font_file.as_deref(), &brand_dir),
    });
    let page_name = format!("card-{key}.html");
    let pages = directory.join("pages");
    std::fs::create_dir_all(&pages).map_err(|_| "Could not prepare the card page.".to_owned())?;
    std::fs::write(pages.join(&page_name), template.compose(&card, &format!("https://{BRAND_HOST}")))
        .map_err(|_| "Could not prepare the card page.".to_owned())?;
    let user_data = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("card-renderer");
    let rendered = render(&user_data, pages.clone(), page_name.clone(), brand_dir, width, height);
    let _ = std::fs::remove_file(pages.join(&page_name));
    let bytes = rendered?;
    let pending = directory.join(format!("{}.png", uuid::Uuid::new_v4()));
    std::fs::write(&pending, bytes).map_err(|_| "Could not save the rendered card.".to_owned())?;
    std::fs::rename(&pending, &destination).map_err(|_| "Could not save the rendered card.".to_owned())?;
    Ok(destination)
}

#[cfg(windows)]
fn render(
    user_data: &Path,
    card_dir: PathBuf,
    page_name: String,
    brand_dir: PathBuf,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    renderer::render_png(
        user_data,
        renderer::RenderJob { card_dir, page_name, brand_dir, width, height },
    )
}

#[cfg(not(windows))]
fn render(
    _user_data: &Path,
    _card_dir: PathBuf,
    _page_name: String,
    _brand_dir: PathBuf,
    _width: u32,
    _height: u32,
) -> Result<Vec<u8>, String> {
    Err("Brand cards render with WebView2 and are only available on Windows.".to_owned())
}

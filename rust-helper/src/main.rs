//! CLI helper for wallpaper analysis, Plasma appearance changes, and theme management.

use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Seek;
use std::io::{self, BufReader, Cursor};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod theme_install;
mod updates;

use theme_install::*;
use updates::*;

// Shared data structures for palettes, commands, and persisted appearance state.
#[derive(Clone, Serialize)]
struct Palette {
    index: usize,
    name: String,
    panel: String,
    background: String,
    surface: String,
    accent: String,
    selection: String,
    foreground: String,
    selection_foreground: String,
}

fn is_generated_theme_id(theme_id: &str) -> bool {
    theme_id == GENERATED_THEME_ID || theme_id.starts_with(&format!("{GENERATED_THEME_ID}-"))
}

fn generated_theme_id_for_scheme(scheme_id: &str) -> String {
    let suffix = scheme_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    format!("{GENERATED_THEME_ID}-{suffix}")
}

#[derive(Serialize)]
struct AnalyzeResult {
    action: &'static str,
    palettes: Vec<Palette>,
    source_type: String,
    sample_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnalysisType {
    Auto,
    Image,
    Gif,
    Video,
    Slideshow,
}

impl AnalysisType {
    fn parse(value: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        match value.unwrap_or("auto") {
            "auto" => Ok(Self::Auto),
            "image" => Ok(Self::Image),
            "gif" => Ok(Self::Gif),
            "video" => Ok(Self::Video),
            "slideshow" => Ok(Self::Slideshow),
            _ => Err("Analysis type must be auto, image, gif, video, or slideshow".into()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Image => "image",
            Self::Gif => "gif",
            Self::Video => "video",
            Self::Slideshow => "slideshow",
        }
    }
}

#[derive(Serialize)]
struct StoreTheme {
    name: String,
    summary: String,
    author: String,
    detail_url: String,
}

#[derive(Serialize)]
struct StoreThemesResult {
    action: &'static str,
    search_color: String,
    themes: Vec<StoreTheme>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    store_search_url: Option<String>,
}

#[derive(Deserialize)]
struct WallpaperVideoEntry {
    filename: String,
    #[serde(default)]
    enabled: bool,
}

#[derive(Serialize)]
struct ApplyResult {
    action: &'static str,
    name: String,
}

#[derive(Serialize)]
struct RemoveResult {
    action: &'static str,
    removed_schemes: usize,
    removed_theme: bool,
}

#[derive(Serialize)]
struct InstallThemeResult {
    action: &'static str,
    theme_type: String,
    themes: Vec<InstalledTheme>,
}

#[derive(Serialize)]
struct InstalledTheme {
    name: String,
    install_path: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct UserState {
    previous_color_scheme: Option<String>,
    previous_desktop_theme: Option<String>,
    #[serde(default)]
    previous_accent_color: Option<String>,
    #[serde(default)]
    previous_inactive_header_background: Option<String>,
    #[serde(default)]
    previous_inactive_header_alternate: Option<String>,
    #[serde(default)]
    appearance_settings_captured: bool,
}

const GENERATED_THEME_ID: &str = "wallpaperpalette-generated";
const WIDGET_OWNER_MARKER: &str = "org.example.plasmacolorpicker";
const LEGACY_WIDGET_OWNER_MARKER: &str = "org.example.wallpaperpalette";

#[derive(Serialize)]
struct DiagnosticCheck {
    id: &'static str,
    ok: bool,
    details: String,
}

#[derive(Serialize)]
struct DiagnosticResult {
    action: &'static str,
    ok: bool,
    checks: Vec<DiagnosticCheck>,
}

// Parse helper commands and serialize their results for the QML widget.
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("analyze") => {
            let mode = AnalysisType::parse(args.next().as_deref())?;
            if args.next().is_some() {
                return Err("Usage: plasmacolorpicker-helper analyze [auto|image|gif|video|slideshow]".into());
            }
            let (palettes, sample_count, source_type) = analyze_wallpaper(mode)?;
            println!(
                "{}",
                serde_json::to_string(&AnalyzeResult {
                    action: "analyze",
                    palettes,
                    source_type: source_type.to_string(),
                    sample_count,
                })?
            );
        }
        Some(action @ ("icons" | "cursors")) => {
            let index = args
                .next()
                .ok_or_else(|| format!("Usage: plasmacolorpicker-helper {action} <0|1|2>"))?
                .parse::<usize>()?;
            let mode = AnalysisType::parse(args.next().as_deref())?;
            if index > 2 || args.next().is_some() {
                return Err("Palette index must be 0, 1, or 2".into());
            }
            let (palettes, _, _) = analyze_wallpaper(mode)?;
            let search_color = hue_search_term(&palettes[index].accent)?;
            let (category, action) = if action == "icons" {
                (132, "icons")
            } else {
                (107, "cursors")
            };
            let (themes, catalog_error, store_search_url) =
                match search_store_themes(&search_color, category) {
                    Ok(themes) => (themes, None, None),
                    Err(error) => (
                        Vec::new(),
                        Some(error.to_string()),
                        Some(format!(
                            "https://store.kde.org/browse?cat={category}&search={search_color}"
                        )),
                    ),
                };
            println!(
                "{}",
                serde_json::to_string(&StoreThemesResult {
                    action,
                    search_color,
                    themes,
                    catalog_error,
                    store_search_url,
                })?
            );
        }
        Some("install-theme") => {
            let theme_type = args
                .next()
                .ok_or("Usage: plasmacolorpicker-helper install-theme <icons|cursors> <archive>")?;
            let archive = args
                .next()
                .ok_or("Usage: plasmacolorpicker-helper install-theme <icons|cursors> <archive>")?;
            if args.next().is_some() {
                return Err("Usage: plasmacolorpicker-helper install-theme <icons|cursors> <archive>".into());
            }
            println!(
                "{}",
                serde_json::to_string(&install_theme(&theme_type, &archive)?)?
            );
        }
        Some("diagnose") => {
            println!("{}", serde_json::to_string(&diagnose())?);
        }
        Some("check-updates") => {
            let baseline = args
                .next()
                .ok_or("Usage: plasmacolorpicker-helper check-updates <baseline-file>")?;
            if args.next().is_some() {
                return Err("Usage: plasmacolorpicker-helper check-updates <baseline-file>".into());
            }
            println!(
                "{}",
                serde_json::to_string(&check_repository_updates(Path::new(&baseline))?)?
            );
        }
        Some("remove") => {
            if args.next().is_some() {
                return Err("Usage: plasmacolorpicker-helper remove".into());
            }
            println!("{}", serde_json::to_string(&remove_owned_palettes()?)?);
        }
        Some("apply") => {
            let index = args
                .next()
                .ok_or("Usage: plasmacolorpicker-helper apply <0|1|2>")?
                .parse::<usize>()?;
            let mode = AnalysisType::parse(args.next().as_deref())?;
            if index > 2 || args.next().is_some() {
                return Err("Palette index must be 0, 1, or 2".into());
            }
            let (palettes, _, _) = analyze_wallpaper(mode)?;
            apply_palette(&palettes[index])?;
        }
        _ => {
            return Err(
                "Usage: plasmacolorpicker-helper analyze [TYPE] | icons INDEX [TYPE] | cursors INDEX [TYPE] | install-theme <icons|cursors> <archive> | check-updates <baseline-file> | diagnose | apply INDEX [TYPE] | remove".into(),
            )
        }
    }
    Ok(())
}

// Search KDE Store for icon and cursor themes matching a palette.
fn hue_search_term(color: &str) -> Result<String, Box<dyn std::error::Error>> {
    if color.len() != 7 || !color.starts_with('#') {
        return Err("Expected an RGB color in #RRGGBB format".into());
    }
    let value = u32::from_str_radix(&color[1..], 16)?;
    let rgb = (
        f64::from((value >> 16) & 0xff) / 255.0,
        f64::from((value >> 8) & 0xff) / 255.0,
        f64::from(value & 0xff) / 255.0,
    );
    let (hue, saturation, _) = rgb_to_hsl(rgb.0, rgb.1, rgb.2);
    if saturation < 0.12 {
        return Ok("dark".to_string());
    }
    Ok(match hue {
        h if !(15.0..345.0).contains(&h) => "red",
        h if h < 38.0 => "orange",
        h if h < 75.0 => "yellow",
        h if h < 165.0 => "green",
        h if h < 195.0 => "teal",
        h if h < 255.0 => "blue",
        h if h < 290.0 => "purple",
        h if h < 345.0 => "pink",
        _ => "red",
    }
    .to_string())
}

fn search_store_themes(
    color: &str,
    category: u16,
) -> Result<Vec<StoreTheme>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.kde-look.org/ocs/v1/content/data?categories={category}&format=json&pagesize=12&search={color}"
    );
    let response: serde_json::Value = ureq::get(&url)
        .timeout(Duration::from_secs(12))
        .call()?
        .into_json()?;
    parse_store_themes_response(&response)
}

fn parse_store_themes_response(
    response: &serde_json::Value,
) -> Result<Vec<StoreTheme>, Box<dyn std::error::Error>> {
    if response.get("status").and_then(serde_json::Value::as_str) != Some("ok") {
        let status_code = response
            .get("statuscode")
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let message = response
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("the response did not contain a status message");
        return Err(
            format!("KDE Store catalog API returned status {status_code}: {message}").into(),
        );
    }
    let items = response
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or("KDE Store returned an invalid theme list")?;
    let themes = items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.trim();
            let detail_url = item.get("detailpage")?.as_str()?;
            let summary = item
                .get("summary")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim();
            let author = item
                .get("personid")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim();
            let id = detail_url.strip_prefix("https://store.kde.org/p/")?;
            if name.is_empty() || id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some(StoreTheme {
                name: name.to_string(),
                summary: summary.to_string(),
                author: author.to_string(),
                detail_url: detail_url.to_string(),
            })
        })
        .collect();
    Ok(themes)
}

// Read the active wallpaper source and analyze still images, slideshows, and video.
fn current_wallpaper() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let (plugin, settings) = active_wallpaper_settings()?;
    let keys = if plugin == "org.kde.slideshow" {
        ["Image", "MediaSource", "Video", "Source", "File", "Path"]
    } else {
        ["MediaSource", "Image", "Video", "Source", "File", "Path"]
    };
    let path = keys
        .into_iter()
        .filter_map(|key| settings.get(key))
        .filter_map(|value| local_path(value))
        .find(|path| path.is_file());
    if let Some(path) = path {
        return Ok(path);
    }
    if let Some(path) = configured_video_paths(&settings)?
        .into_iter()
        .find(|path| path.is_file())
    {
        return Ok(path);
    }
    if plugin == "org.kde.slideshow" {
        if let Some(path) = slideshow_image_paths(&settings)?.into_iter().next() {
            return Ok(path);
        }
    }
    Err(format!("No readable wallpaper source was found for plugin {plugin}").into())
}

fn configured_video_paths(
    settings: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let Some(value) = settings.get("VideoUrls") else {
        return Ok(Vec::new());
    };
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }

    let entries: Vec<WallpaperVideoEntry> = serde_json::from_str(value).map_err(|error| {
        format!("Cannot parse the wallpaper plugin's VideoUrls setting: {error}")
    })?;
    Ok(entries
        .into_iter()
        .filter(|entry| entry.enabled)
        .filter_map(|entry| local_path(&entry.filename))
        .collect())
}

fn active_wallpaper_settings(
) -> Result<(String, std::collections::BTreeMap<String, String>), Box<dyn std::error::Error>> {
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("Cannot find the user's configuration directory")?;
    let config_path = config_home.join("plasma-org.kde.plasma.desktop-appletsrc");
    let config = fs::read_to_string(&config_path).map_err(|error| {
        format!(
            "Cannot read Plasma desktop configuration {}: {error}",
            config_path.display()
        )
    })?;

    parse_wallpaper_settings(&config)
}

fn parse_wallpaper_settings(
    config: &str,
) -> Result<(String, std::collections::BTreeMap<String, String>), Box<dyn std::error::Error>> {
    let mut current_section = String::new();
    let mut sections =
        std::collections::BTreeMap::<String, std::collections::BTreeMap<String, String>>::new();
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line.to_string();
            sections.entry(current_section.clone()).or_default();
            if line.starts_with("[Containments][") && !line.contains("][Wallpaper]") {
                continue;
            }
        }
        if let Some((key, value)) = line.split_once('=') {
            if !current_section.is_empty() {
                sections
                    .entry(current_section.clone())
                    .or_default()
                    .insert(key.to_string(), value.to_string());
            }
        }
    }

    let is_desktop = |section: &str, values: &std::collections::BTreeMap<String, String>| {
        section.starts_with("[Containments][")
            && section.matches("][").count() == 1
            && values.get("formfactor").map(String::as_str).unwrap_or("0") == "0"
    };
    let selected = sections
        .iter()
        .find(|(section, values)| {
            is_desktop(section, values)
                && values.get("lastScreen").is_some_and(|screen| screen == "0")
        })
        .or_else(|| {
            sections
                .iter()
                .find(|(section, values)| is_desktop(section, values))
        })
        .ok_or("No Plasma desktop containment was found")?;
    let containment = selected.0.as_str();
    let plugin = selected
        .1
        .get("wallpaperplugin")
        .map(String::as_str)
        .unwrap_or("org.kde.image");
    let wallpaper_section = format!("{containment}[Wallpaper][{plugin}][General]");
    let settings = sections
        .get(&wallpaper_section)
        .ok_or_else(|| format!("No settings were found for active wallpaper plugin {plugin}"))?;
    Ok((plugin.to_string(), settings.clone()))
}

fn local_file_path(value: &str) -> Option<PathBuf> {
    local_path(value)
}

fn local_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let value = value.strip_prefix("file://").unwrap_or(value);
    if value.contains("://") {
        return None;
    }
    let decoded = percent_decode(value)?;
    Some(PathBuf::from(decoded))
}

fn analyze_wallpaper(
    mode: AnalysisType,
) -> Result<(Vec<Palette>, usize, &'static str), Box<dyn std::error::Error>> {
    if mode == AnalysisType::Slideshow {
        let (plugin, settings) = active_wallpaper_settings()?;
        if plugin != "org.kde.slideshow" {
            return Err("Slideshow mode requires the built-in Plasma slideshow wallpaper".into());
        }
        let paths = slideshow_image_paths(&settings)?;
        let frames = paths
            .iter()
            .map(|path| -> Result<RgbaImage, Box<dyn std::error::Error>> {
                let reader = ImageReader::open(path)?;
                let image = reader.with_guessed_format()?.decode()?;
                Ok(image.thumbnail(320, 320).to_rgba8())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (palettes, count) = palettes_from_frames(frames)?;
        return Ok((palettes, count, "slideshow"));
    }

    let path = current_wallpaper()?;
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let selected_mode = match mode {
        AnalysisType::Auto if matches!(extension.as_str(), "gif" | "webp") => AnalysisType::Gif,
        AnalysisType::Auto if is_video_extension(&extension) => AnalysisType::Video,
        AnalysisType::Auto => AnalysisType::Image,
        explicit => explicit,
    };

    let frames = match selected_mode {
        AnalysisType::Gif => {
            if !matches!(extension.as_str(), "gif" | "webp") {
                return Err(
                    "Animated image mode requires a GIF or animated WebP wallpaper source".into(),
                );
            }
            sample_animated_frames(&path, &extension)?
        }
        AnalysisType::Video => {
            if !is_video_extension(&extension) {
                return Err(
                    "Video mode requires the active wallpaper source to be a video file".into(),
                );
            }
            sample_video_frames(&path)?
        }
        AnalysisType::Slideshow => unreachable!(),
        AnalysisType::Auto | AnalysisType::Image => {
            if is_video_extension(&extension) || matches!(extension.as_str(), "gif" | "webp") {
                return Err("Select Auto, Animated image, or Video for this wallpaper".into());
            }
            let image = ImageReader::open(&path)?.with_guessed_format()?.decode()?;
            vec![image.thumbnail(320, 320).to_rgba8()]
        }
    };
    if frames.is_empty() {
        return Err("No frames could be decoded from the wallpaper".into());
    }

    let (palettes, count) = if selected_mode == AnalysisType::Video {
        palettes_from_video_frames(frames)?
    } else {
        palettes_from_frames(frames)?
    };
    Ok((palettes, count, selected_mode.as_str()))
}

fn palettes_from_frames(
    frames: Vec<RgbaImage>,
) -> Result<(Vec<Palette>, usize), Box<dyn std::error::Error>> {
    if frames.is_empty() {
        return Err("No frames could be decoded from the wallpaper".into());
    }
    let sample = combine_sample_frames(&frames);
    let colors = estimate_wallpaper_colors(&sample);
    let palettes = colors
        .into_iter()
        .enumerate()
        .map(|(index, (hue, saturation, lightness, region_hue))| {
            palette_from_color(index, hue, saturation, lightness, region_hue)
        })
        .collect();
    Ok((palettes, frames.len()))
}

fn palettes_from_video_frames(
    frames: Vec<RgbaImage>,
) -> Result<(Vec<Palette>, usize), Box<dyn std::error::Error>> {
    if frames.is_empty() {
        return Err("No frames could be decoded from the wallpaper".into());
    }

    let sample_count = frames.len();
    let total_pixels = frames
        .iter()
        .map(|frame| u64::from(frame.width()) * u64::from(frame.height()))
        .sum::<u64>();
    let chromatic_pixels = frames
        .iter()
        .flat_map(RgbaImage::pixels)
        .filter(|pixel| {
            let [red, green, blue, alpha] = pixel.0;
            if alpha < 128 {
                return false;
            }
            let (_, saturation, lightness) = rgb_to_hsl(
                f64::from(red) / 255.0,
                f64::from(green) / 255.0,
                f64::from(blue) / 255.0,
            );
            saturation >= 0.15 && (0.035..=0.94).contains(&lightness)
        })
        .count() as u64;

    let palettes = if chromatic_pixels as f64 / (total_pixels.max(1) as f64) < 0.01 {
        (0..3).map(neutral_video_palette).collect()
    } else {
        estimate_wallpaper_colors(&combine_sample_frames(&frames))
            .into_iter()
            .enumerate()
            .map(|(index, (hue, saturation, lightness, region_hue))| {
                palette_from_color(index, hue, saturation, lightness, region_hue)
            })
            .collect()
    };
    Ok((palettes, sample_count))
}

fn neutral_video_palette(index: usize) -> Palette {
    let (panel, background, surface, accent, selection) = match index {
        0 => ("#242428", "#19191C", "#303035", "#A1A1AA", "#52525B"),
        1 => ("#29292D", "#1D1D21", "#36363B", "#D4D4D8", "#71717A"),
        _ => ("#202023", "#161618", "#2B2B2F", "#737373", "#404040"),
    };
    let selection_rgb = (
        f64::from(u8::from_str_radix(&selection[1..3], 16).expect("valid palette color")) / 255.0,
        f64::from(u8::from_str_radix(&selection[3..5], 16).expect("valid palette color")) / 255.0,
        f64::from(u8::from_str_radix(&selection[5..7], 16).expect("valid palette color")) / 255.0,
    );
    Palette {
        index,
        name: format!("PlasmaColorPicker {:02}", index + 1),
        panel: panel.to_string(),
        background: background.to_string(),
        surface: surface.to_string(),
        accent: accent.to_string(),
        selection: selection.to_string(),
        foreground: "#F4F4F5".to_string(),
        selection_foreground: contrast_foreground(selection_rgb),
    }
}

fn slideshow_image_paths(
    settings: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    if let Some(paths) = settings.get("SlidePaths") {
        for value in paths.split(',') {
            if let Some(path) = local_path(value) {
                collect_slideshow_files(&path, 0, &mut files)?;
                if files.len() >= 256 {
                    break;
                }
            }
        }
    }
    if files.is_empty() {
        if let Some(image) = settings.get("Image").and_then(|value| local_path(value)) {
            if image.is_file() {
                files.push(image);
            }
        }
    }
    files.sort();
    files.dedup();
    if files.len() > 8 {
        let length = files.len();
        files = (0..8)
            .map(|index| files[index * (length - 1) / 7].clone())
            .collect();
    }
    Ok(files)
}

fn collect_slideshow_files(path: &Path, depth: usize, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if files.len() >= 256 || depth > 8 {
        return Ok(());
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.is_file() {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(
            extension.as_str(),
            "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif"
        ) {
            files.push(path.to_path_buf());
        }
    } else if metadata.is_dir() {
        let mut entries = fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        entries.sort();
        for entry in entries {
            collect_slideshow_files(&entry, depth + 1, files)?;
            if files.len() >= 256 {
                break;
            }
        }
    }
    Ok(())
}

fn is_video_extension(extension: &str) -> bool {
    matches!(
        extension,
        "mp4"
            | "webm"
            | "mkv"
            | "mov"
            | "m4v"
            | "avi"
            | "wmv"
            | "flv"
            | "mpeg"
            | "mpg"
            | "ogv"
            | "3gp"
            | "ts"
            | "mts"
            | "m2ts"
    )
}

fn sample_animated_frames(
    path: &Path,
    extension: &str,
) -> Result<Vec<RgbaImage>, Box<dyn std::error::Error>> {
    let reader = BufReader::new(fs::File::open(path)?);
    if extension == "webp" {
        return sample_animation_frames(WebPDecoder::new(reader)?);
    }
    sample_animation_frames(GifDecoder::new(reader)?)
}

fn sample_animation_frames<'a, D>(decoder: D) -> Result<Vec<RgbaImage>, Box<dyn std::error::Error>>
where
    D: AnimationDecoder<'a>,
{
    const MAX_FRAMES: usize = 8;
    let mut samples = Vec::with_capacity(MAX_FRAMES);
    let mut random_state = 0x9E37_79B9_u64;
    for frame in decoder.into_frames() {
        let frame = frame?;
        let (delay_numerator, delay_denominator) = frame.delay().numer_denom_ms();
        let duration_ms = if delay_denominator == 0 {
            100.0
        } else {
            (f64::from(delay_numerator) / f64::from(delay_denominator)).max(10.0)
        };
        let image = image::DynamicImage::ImageRgba8(frame.into_buffer())
            .thumbnail(320, 320)
            .to_rgba8();
        random_state ^= random_state << 13;
        random_state ^= random_state >> 7;
        random_state ^= random_state << 17;
        let random_unit = ((random_state >> 11) as f64 + 1.0) / ((1_u64 << 53) as f64 + 1.0);
        let priority = random_unit.ln() / duration_ms;
        if samples.len() < MAX_FRAMES {
            samples.push((priority, image));
        } else if let Some((minimum_index, (minimum_priority, _))) = samples
            .iter()
            .enumerate()
            .min_by(|(_, (left, _)), (_, (right, _))| left.total_cmp(right))
        {
            if priority > *minimum_priority {
                samples[minimum_index] = (priority, image);
            }
        }
    }
    Ok(samples.into_iter().map(|(_, image)| image).collect())
}

fn sample_video_frames(path: &Path) -> Result<Vec<RgbaImage>, Box<dyn std::error::Error>> {
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .map_err(|error| format!("Video analysis requires ffprobe from FFmpeg: {error}"))?;
    if !probe.status.success() {
        return Err(format!(
            "ffprobe could not read the video: {}",
            String::from_utf8_lossy(&probe.stderr).trim()
        )
        .into());
    }
    let duration = String::from_utf8_lossy(&probe.stdout)
        .trim()
        .parse::<f64>()?;
    if !duration.is_finite() || duration <= 0.0 {
        return Err("The video has no valid duration to sample".into());
    }

    let mut frames = Vec::new();
    let mut previous_timestamp: Option<f64> = None;
    for fraction in [0.08, 0.24, 0.40, 0.56, 0.72, 0.88] {
        let timestamp = (duration * fraction).min((duration - 0.02).max(0.0));
        if previous_timestamp.is_some_and(|previous| (timestamp - previous).abs() < 0.001) {
            continue;
        }
        previous_timestamp = Some(timestamp);
        let output = Command::new("ffmpeg")
            .args(["-v", "error", "-ss"])
            .arg(format!("{timestamp:.3}"))
            .arg("-i")
            .arg(path)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "scale=320:320:force_original_aspect_ratio=decrease",
                "-f",
                "image2pipe",
                "-vcodec",
                "png",
                "pipe:1",
            ])
            .output()
            .map_err(|error| format!("Video analysis requires FFmpeg: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "FFmpeg could not decode a frame at {timestamp:.3}s: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        if output.stdout.is_empty() {
            continue;
        }
        let image = ImageReader::new(Cursor::new(output.stdout))
            .with_guessed_format()?
            .decode()?
            .thumbnail(320, 320)
            .to_rgba8();
        frames.push(image);
    }
    if frames.is_empty() {
        return Err("FFmpeg could not decode any video frames".into());
    }
    Ok(frames)
}

fn combine_sample_frames(frames: &[RgbaImage]) -> RgbaImage {
    let width = frames.iter().map(RgbaImage::width).max().unwrap_or(1);
    let height = frames.iter().map(RgbaImage::height).sum::<u32>().max(1);
    let mut combined = RgbaImage::new(width, height);
    let mut y_offset = 0;
    for frame in frames {
        for (x, y, pixel) in frame.enumerate_pixels() {
            combined.put_pixel(x, y + y_offset, *pixel);
        }
        y_offset += frame.height();
    }
    combined
}

// Check required desktop tools and filesystem/configuration conditions.
fn diagnose() -> DiagnosticResult {
    let wallpaper = current_wallpaper().map_err(|error| error.to_string());
    let mut checks = vec![DiagnosticCheck {
        id: "wallpaper",
        ok: wallpaper.is_ok(),
        details: wallpaper
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| error.clone()),
    }];

    let image_check = wallpaper
        .as_ref()
        .map_err(|error| error.to_string())
        .and_then(|path| -> Result<String, String> {
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if extension == "gif" || extension == "webp" {
                sample_animated_frames(path, &extension)
                    .map(|frames| format!("Decoded {} animation frames", frames.len()))
                    .map_err(|error| error.to_string())
            } else if is_video_extension(&extension) {
                sample_video_frames(path)
                    .map(|frames| format!("Decoded {} video frames", frames.len()))
                    .map_err(|error| error.to_string())
            } else {
                ImageReader::open(path)
                    .map_err(|error| error.to_string())?
                    .with_guessed_format()
                    .map_err(|error| error.to_string())?
                    .decode()
                    .map(|_| format!("Decoded {}", path.display()))
                    .map_err(|error| error.to_string())
            }
        });
    checks.push(DiagnosticCheck {
        id: "image",
        ok: image_check.is_ok(),
        details: image_check.unwrap_or_else(|error| error),
    });

    let apply_command = find_executable("plasma-apply-colorscheme");
    checks.push(DiagnosticCheck {
        id: "apply-command",
        ok: apply_command.is_some(),
        details: apply_command
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "plasma-apply-colorscheme was not found in PATH".to_string()),
    });

    let theme_command = find_executable("plasma-apply-desktoptheme");
    checks.push(DiagnosticCheck {
        id: "desktop-theme-command",
        ok: theme_command.is_some(),
        details: theme_command
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "plasma-apply-desktoptheme was not found in PATH".to_string()),
    });

    let archive_command = find_executable("unar").or_else(|| find_executable("7z"));
    checks.push(DiagnosticCheck {
        id: "archive-command",
        ok: archive_command.is_some(),
        details: archive_command
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| {
                "Neither unar nor 7z was found in PATH; archive themes can be installed manually"
                    .to_string()
            }),
    });

    let current_theme = active_desktop_theme();
    checks.push(DiagnosticCheck {
        id: "desktop-theme",
        ok: current_theme.as_ref().is_ok_and(|theme| theme.is_some()),
        details: match current_theme {
            Ok(Some(theme)) => theme,
            Ok(None) => "No active Plasma desktop theme was found".into(),
            Err(error) => error.to_string(),
        },
    });

    let output_dir = user_colors_dir();
    let output_check = output_dir.and_then(check_directory_writable);
    checks.push(DiagnosticCheck {
        id: "output-directory",
        ok: output_check.is_ok(),
        details: output_check
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| error.to_string()),
    });

    let theme_output_dir = user_data_dir()
        .map(|path| path.join("plasma/desktoptheme").to_path_buf())
        .and_then(check_directory_writable);
    checks.push(DiagnosticCheck {
        id: "desktop-theme-directory",
        ok: theme_output_dir.is_ok(),
        details: theme_output_dir
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| error.to_string()),
    });

    DiagnosticResult {
        action: "diagnose",
        ok: checks.iter().all(|check| check.ok),
        checks,
    }
}

// Resolve XDG paths and read/write the helper's saved state and KDE settings.
fn find_executable(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

fn resolve_xdg_user_dir(
    configured: Option<PathBuf>,
    home: Option<PathBuf>,
    fallback: &str,
) -> io::Result<PathBuf> {
    configured
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(fallback)))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Cannot determine the user's XDG directory",
            )
        })
}

fn user_data_dir() -> io::Result<PathBuf> {
    resolve_xdg_user_dir(
        env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
        ".local/share",
    )
}

fn user_colors_dir() -> io::Result<PathBuf> {
    Ok(user_data_dir()?.join("color-schemes"))
}

fn user_config_dir() -> io::Result<PathBuf> {
    resolve_xdg_user_dir(
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
        ".config",
    )
}

fn user_state_path() -> io::Result<PathBuf> {
    Ok(user_config_dir()?
        .join("wallpaper-palette")
        .join("state.json"))
}

fn read_user_state() -> io::Result<Option<UserState>> {
    let path = user_state_path()?;
    match fs::read(&path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_user_state(state: &UserState) -> io::Result<()> {
    let path = user_state_path()?;
    let parent = path.parent().expect("state file has a parent directory");
    fs::create_dir_all(parent)?;
    let temporary_path = parent.join(format!("state-{}.tmp", std::process::id()));
    fs::write(
        &temporary_path,
        serde_json::to_vec_pretty(state).map_err(io::Error::other)?,
    )?;
    fs::rename(temporary_path, path)
}

fn read_config_value(path: &Path, group: &str, key: &str) -> io::Result<Option<String>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let target_group = format!("[{group}]");
    let mut in_group = false;
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            in_group = line == target_group;
        } else if in_group {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim() == key {
                    return Ok(Some(value.trim().to_string()));
                }
            }
        }
    }
    Ok(None)
}

fn active_color_scheme() -> io::Result<Option<String>> {
    Ok(read_config_value(
        &user_config_dir()?.join("kdeglobals"),
        "General",
        "ColorScheme",
    )?)
}

fn active_accent_color() -> io::Result<Option<String>> {
    Ok(read_config_value(
        &user_config_dir()?.join("kdeglobals"),
        "General",
        "AccentColor",
    )?)
}

fn inactive_header_color(key: &str) -> io::Result<Option<String>> {
    read_config_value(
        &user_config_dir()?.join("kdeglobals"),
        "Colors:Header][Inactive",
        key,
    )
}

fn notify_appearance_changes(scheme_id: &str, accent: &str, theme_id: &str) -> io::Result<()> {
    notify_kde_setting("kdeglobals", "General", "ColorScheme", scheme_id)?;
    notify_kde_setting("kdeglobals", "General", "AccentColor", &kde_rgb(accent))?;
    notify_kde_setting("plasmarc", "Theme", "name", theme_id)?;

    let active_scheme = active_color_scheme()?.unwrap_or_default();
    let active_theme = active_desktop_theme()?.unwrap_or_default();
    if active_scheme != scheme_id || active_theme != theme_id {
        return Err(io::Error::other(format!(
            "KDE appearance settings did not settle: color scheme={active_scheme}, Plasma theme={active_theme}"
        )));
    }
    Ok(())
}

fn notify_kde_setting(file: &str, group: &str, key: &str, value: &str) -> io::Result<()> {
    write_kde_setting(file, &[group], key, Some(value))
}

fn delete_kde_setting(file: &str, group: &str, key: &str) -> io::Result<()> {
    write_kde_setting(file, &[group], key, None)
}

fn write_kde_setting(
    file: &str,
    groups: &[&str],
    key: &str,
    value: Option<&str>,
) -> io::Result<()> {
    let mut command = Command::new("kwriteconfig6");
    command.args(["--notify", "--file", file]);
    for group in groups {
        command.args(["--group", group]);
    }
    command.args(["--key", key]);
    if let Some(value) = value {
        command.arg(value);
    } else {
        command.arg("--delete");
    }
    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "KDE could not update {file} setting {}/{key}: {}",
            groups.join("/"),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn verify_active_scheme_colors(scheme_id: &str) -> io::Result<()> {
    let scheme_path = user_colors_dir()?.join(format!("{scheme_id}.colors"));
    let scheme = fs::read_to_string(&scheme_path)?;
    let kdeglobals = user_config_dir()?.join("kdeglobals");
    let mut group = String::new();
    for line in scheme.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            group = line[1..line.len() - 1].to_string();
        } else if group.starts_with("Colors:") || group == "WM" {
            if let Some((key, expected)) = line.split_once('=') {
                let active =
                    read_config_value(&kdeglobals, &group, key.trim())?.unwrap_or_default();
                if active != expected.trim() {
                    return Err(io::Error::other(format!(
                        "KDE kept a stale color in {group}/{key}: expected {}, found {active}",
                        expected.trim()
                    )));
                }
            }
        }
    }
    Ok(())
}

fn apply_inactive_header_colors(palette: &Palette) -> io::Result<()> {
    let values = [
        ("BackgroundNormal", kde_rgb(&palette.panel)),
        ("BackgroundAlternate", kde_rgb(&palette.background)),
    ];
    for (key, value) in values {
        let output = Command::new("kwriteconfig6")
            .args([
                "--notify",
                "--file",
                "kdeglobals",
                "--group",
                "Colors:Header",
                "--group",
                "Inactive",
                "--key",
                key,
            ])
            .arg(&value)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "KDE could not update inactive header color {key}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let actual = inactive_header_color(key)?.unwrap_or_default();
        if actual != value {
            return Err(io::Error::other(format!(
                "KDE reported success, but inactive header color {key} is {actual}"
            )));
        }
    }
    Ok(())
}

fn restore_appearance_settings(
    state: Option<&UserState>,
    owned_schemes: &[PathBuf],
) -> io::Result<()> {
    if let Some(state) = state.filter(|state| state.appearance_settings_captured) {
        write_kde_setting(
            "kdeglobals",
            &["General"],
            "AccentColor",
            state.previous_accent_color.as_deref(),
        )?;
        write_kde_setting(
            "kdeglobals",
            &["Colors:Header", "Inactive"],
            "BackgroundNormal",
            state.previous_inactive_header_background.as_deref(),
        )?;
        write_kde_setting(
            "kdeglobals",
            &["Colors:Header", "Inactive"],
            "BackgroundAlternate",
            state.previous_inactive_header_alternate.as_deref(),
        )?;
        return Ok(());
    }

    let accent = active_accent_color()?;
    let inactive_normal = inactive_header_color("BackgroundNormal")?;
    let inactive_alternate = inactive_header_color("BackgroundAlternate")?;
    let mut clear_accent = false;
    let mut clear_inactive_normal = false;
    let mut clear_inactive_alternate = false;
    for scheme in owned_schemes {
        let contents = fs::read_to_string(scheme)?;
        clear_accent |= accent.as_ref().is_some_and(|value| {
            read_ini_value(&contents, "Colors:Header", "ForegroundActive").as_ref() == Some(value)
        });
        clear_inactive_normal |= inactive_normal.as_ref().is_some_and(|value| {
            read_ini_value(&contents, "Colors:Header][Inactive", "BackgroundNormal").as_ref()
                == Some(value)
        });
        clear_inactive_alternate |= inactive_alternate.as_ref().is_some_and(|value| {
            read_ini_value(&contents, "Colors:Header][Inactive", "BackgroundAlternate").as_ref()
                == Some(value)
        });
    }
    if clear_accent {
        delete_kde_setting("kdeglobals", "General", "AccentColor")?;
    }
    if clear_inactive_normal {
        write_kde_setting(
            "kdeglobals",
            &["Colors:Header", "Inactive"],
            "BackgroundNormal",
            None,
        )?;
    }
    if clear_inactive_alternate {
        write_kde_setting(
            "kdeglobals",
            &["Colors:Header", "Inactive"],
            "BackgroundAlternate",
            None,
        )?;
    }
    Ok(())
}

fn verify_active_inactive_header_colors(scheme_id: &str) -> io::Result<()> {
    let scheme_path = user_colors_dir()?.join(format!("{scheme_id}.colors"));
    let scheme = fs::read_to_string(scheme_path)?;
    let scheme_group = "Colors:Header][Inactive";
    let global_path = user_config_dir()?.join("kdeglobals");
    for key in ["BackgroundNormal", "BackgroundAlternate"] {
        let expected = read_ini_value(&scheme, scheme_group, key).unwrap_or_default();
        let active = read_config_value(&global_path, scheme_group, key)?.unwrap_or_default();
        if active != expected {
            return Err(io::Error::other(format!(
                "KDE kept a stale color in {scheme_group}/{key}: expected {expected}, found {active}"
            )));
        }
    }
    Ok(())
}

fn active_desktop_theme() -> io::Result<Option<String>> {
    let config_dir = user_config_dir()?;
    if let Some(theme) = read_config_value(&config_dir.join("plasmarc"), "Theme", "name")?
        .filter(|theme| !theme.is_empty())
    {
        return Ok(Some(theme));
    }

    let Some(look_and_feel_package) =
        read_config_value(&config_dir.join("kdeglobals"), "KDE", "LookAndFeelPackage")?
            .filter(|package| !package.is_empty())
    else {
        return Ok(None);
    };
    for data_dir in plasma_data_dirs() {
        let defaults_path = data_dir
            .join("plasma/look-and-feel")
            .join(&look_and_feel_package)
            .join("contents/defaults");
        let defaults = match fs::read_to_string(defaults_path) {
            Ok(defaults) => defaults,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if let Some(theme) = read_kde_defaults_value(&defaults, "plasmarc", "Theme", "name")
            .filter(|theme| !theme.is_empty())
        {
            return Ok(Some(theme));
        }
    }
    Ok(None)
}

fn read_kde_defaults_value(contents: &str, file: &str, group: &str, key: &str) -> Option<String> {
    let target_group = format!("[{file}][{group}]");
    let mut in_group = false;
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            in_group = line == target_group;
        } else if in_group {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim() == key {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

fn plasma_data_dirs() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Ok(data_home) = user_data_dir() {
        directories.push(data_home);
    }
    if let Some(data_dirs) = env::var_os("XDG_DATA_DIRS") {
        directories.extend(env::split_paths(&data_dirs));
    } else {
        directories.extend([
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]);
    }
    directories
}

fn find_desktop_theme(theme_id: &str) -> io::Result<PathBuf> {
    for data_dir in plasma_data_dirs() {
        let candidate = data_dir.join("plasma/desktoptheme").join(theme_id);
        if candidate.join("metadata.json").is_file() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("Could not find Plasma desktop theme {theme_id}"),
    ))
}

fn check_directory_writable(directory: PathBuf) -> io::Result<PathBuf> {
    fs::create_dir_all(&directory)?;
    let probe_path = directory.join(format!(".wallpaper-palette-check-{}", std::process::id()));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
        .map_err(|error| io::Error::new(error.kind(), error))?;
    fs::remove_file(probe_path)?;
    Ok(directory)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = value.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

// Convert sampled wallpaper colors into palettes with readable foreground colors.
fn palette_from_color(
    index: usize,
    hue: f64,
    saturation: f64,
    lightness: f64,
    region_hue: f64,
) -> Palette {
    let hue = blend_hue(hue, region_hue, 0.2);
    let panel_rgb = hsl_to_rgb(
        hue,
        (saturation * 0.52).clamp(0.15, 0.36),
        (0.15 + lightness * 0.10).clamp(0.16, 0.23),
    );
    let background_rgb = hsl_to_rgb(
        hue,
        (saturation * 0.42).clamp(0.12, 0.30),
        (0.12 + lightness * 0.10).clamp(0.13, 0.19),
    );
    let surface_rgb = hsl_to_rgb(
        hue,
        (saturation * 0.50).clamp(0.15, 0.34),
        (0.18 + lightness * 0.12).clamp(0.20, 0.28),
    );
    let accent = hsl_to_rgb(
        hue,
        saturation.clamp(0.38, 0.68),
        (0.48 + lightness * 0.15).clamp(0.50, 0.65),
    );
    let selection = hsl_to_rgb(
        hue,
        saturation.clamp(0.35, 0.64),
        (0.31 + lightness * 0.12).clamp(0.34, 0.45),
    );
    Palette {
        index,
        name: format!("PlasmaColorPicker {:02}", index + 1),
        panel: hex(panel_rgb),
        background: hex(background_rgb),
        surface: hex(surface_rgb),
        accent: hex(accent),
        selection: hex(selection),
        foreground: "#F4F4F5".to_string(),
        selection_foreground: contrast_foreground(selection),
    }
}

fn estimate_wallpaper_colors(sample: &RgbaImage) -> Vec<(f64, f64, f64, f64)> {
    const HUE_BUCKETS: usize = 72;
    const MIN_SATURATION: f64 = 0.15;
    const MIN_LIGHTNESS: f64 = 0.035;
    const MAX_LIGHTNESS: f64 = 0.94;
    const MIN_HUE_SEPARATION: f64 = 18.0;

    #[derive(Clone, Copy, Default)]
    struct HueBucket {
        weight: f64,
        saturation_weight: f64,
        lightness_weight: f64,
        hue_x: f64,
        hue_y: f64,
    }

    let mut buckets = [HueBucket::default(); HUE_BUCKETS];
    for pixel in sample.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        if alpha < 128 {
            continue;
        }

        let (hue, saturation, lightness) = rgb_to_hsl(
            f64::from(red) / 255.0,
            f64::from(green) / 255.0,
            f64::from(blue) / 255.0,
        );
        if saturation < MIN_SATURATION || !(MIN_LIGHTNESS..=MAX_LIGHTNESS).contains(&lightness) {
            continue;
        }

        let weight = saturation * (0.55 + 0.45 * lightness);
        let bucket_index = ((hue / 360.0 * HUE_BUCKETS as f64).floor() as usize) % HUE_BUCKETS;
        buckets[bucket_index].weight += weight;
        buckets[bucket_index].saturation_weight += saturation * weight;
        buckets[bucket_index].lightness_weight += lightness * weight;
        let radians = hue.to_radians();
        buckets[bucket_index].hue_x += radians.cos() * weight;
        buckets[bucket_index].hue_y += radians.sin() * weight;
    }

    let scores = buckets
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let score = [-1_isize, 0, 1]
                .into_iter()
                .map(|offset| {
                    let neighbor =
                        (index as isize + offset).rem_euclid(HUE_BUCKETS as isize) as usize;
                    buckets[neighbor].weight
                })
                .sum::<f64>();
            score
        })
        .collect::<Vec<_>>();
    let mut peaks = scores
        .iter()
        .enumerate()
        .filter_map(|(index, score)| {
            let weight = buckets[index].weight;
            let left = buckets[(index + HUE_BUCKETS - 1) % HUE_BUCKETS].weight;
            let right = buckets[(index + 1) % HUE_BUCKETS].weight;
            (weight > 0.0 && weight >= left && weight > right).then_some((index, *score))
        })
        .collect::<Vec<_>>();
    peaks.sort_by(|left, right| right.1.total_cmp(&left.1));

    let mut selected: Vec<(f64, f64, f64, f64)> = Vec::new();
    for (index, _) in peaks {
        let hue = (index as f64 + 0.5) * 360.0 / HUE_BUCKETS as f64;
        if selected.iter().all(|(other_hue, _, _, _)| {
            circular_hue_distance(hue, *other_hue) >= MIN_HUE_SEPARATION
        }) {
            let mut weight = 0.0;
            let mut saturation_weight = 0.0;
            let mut lightness_weight = 0.0;
            for offset in [-1_isize, 0, 1] {
                let neighbor = (index as isize + offset).rem_euclid(HUE_BUCKETS as isize) as usize;
                let bucket = buckets[neighbor];
                weight += bucket.weight;
                saturation_weight += bucket.saturation_weight;
                lightness_weight += bucket.lightness_weight;
            }
            if weight > 0.0 {
                let bucket = buckets[index];
                let representative_hue = bucket
                    .hue_y
                    .atan2(bucket.hue_x)
                    .to_degrees()
                    .rem_euclid(360.0);
                let region_hue =
                    strongest_edge_hue(sample, representative_hue).unwrap_or(representative_hue);
                selected.push((
                    representative_hue,
                    saturation_weight / weight,
                    lightness_weight / weight,
                    region_hue,
                ));
            }
            if selected.len() == 3 {
                break;
            }
        }
    }

    if selected.is_empty() {
        selected.extend([(220.0, 0.35, 0.35, 220.0)]);
    }
    while selected.len() < 3 {
        let (hue, saturation, lightness, _region_hue) = selected[0];
        let offset = if selected.len() == 1 { 30.0 } else { -30.0 };
        let derived_hue = (hue + offset).rem_euclid(360.0);
        selected.push((derived_hue, saturation, lightness, derived_hue));
    }

    selected.into_iter().take(3).collect()
}

fn strongest_edge_hue(sample: &RgbaImage, target_hue: f64) -> Option<f64> {
    if sample.width() == 0 || sample.height() == 0 {
        return None;
    }
    let edge_height = (sample.height() / 10).max(1);
    let mut best = None;
    for (start_y, end_y) in [
        (0, edge_height),
        (sample.height() - edge_height, sample.height()),
    ] {
        let mut weight = 0.0;
        let mut hue_x = 0.0;
        let mut hue_y = 0.0;
        for y in start_y..end_y {
            for x in 0..sample.width() {
                let [red, green, blue, alpha] = sample.get_pixel(x, y).0;
                if alpha < 128 {
                    continue;
                }
                let (hue, saturation, lightness) = rgb_to_hsl(
                    f64::from(red) / 255.0,
                    f64::from(green) / 255.0,
                    f64::from(blue) / 255.0,
                );
                if saturation < 0.12
                    || !(0.025..=0.96).contains(&lightness)
                    || circular_hue_distance(hue, target_hue) > 24.0
                {
                    continue;
                }
                let pixel_weight = saturation * (0.5 + lightness);
                let radians = hue.to_radians();
                hue_x += radians.cos() * pixel_weight;
                hue_y += radians.sin() * pixel_weight;
                weight += pixel_weight;
            }
        }
        if weight > best.as_ref().map_or(0.0, |(best_weight, _)| *best_weight) {
            best = Some((weight, hue_y.atan2(hue_x).to_degrees().rem_euclid(360.0)));
        }
    }
    best.map(|(_, hue)| hue)
}

fn blend_hue(base_hue: f64, regional_hue: f64, regional_weight: f64) -> f64 {
    let base = base_hue.to_radians();
    let regional = regional_hue.to_radians();
    let x = base.cos() * (1.0 - regional_weight) + regional.cos() * regional_weight;
    let y = base.sin() * (1.0 - regional_weight) + regional.sin() * regional_weight;
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

fn circular_hue_distance(left: f64, right: f64) -> f64 {
    let difference = (left - right).abs().rem_euclid(360.0);
    difference.min(360.0 - difference)
}

// Generate and activate a Plasma theme and color scheme for the selected palette.
fn apply_palette(palette: &Palette) -> Result<(), Box<dyn std::error::Error>> {
    let colors_dir = user_colors_dir()?;
    fs::create_dir_all(&colors_dir)?;
    let scheme_id = unique_scheme_id(&colors_dir, palette.index)?;
    let scheme_path = colors_dir.join(format!("{scheme_id}.colors"));
    let display_name = scheme_display_name(&scheme_id, palette.index);
    let (state, new_state) = get_or_create_state()?;
    let previous_theme = active_desktop_theme()?;
    fs::write(&scheme_path, colors_file(&display_name, palette))?;
    if let Err(error) = write_user_state(&state) {
        let _ = fs::remove_file(&scheme_path);
        return Err(error.into());
    }

    let generated_theme_id = match install_generated_desktop_theme(&state, palette, &scheme_id) {
        Ok(theme_id) => theme_id,
        Err(error) => {
            let _ = fs::remove_file(&scheme_path);
            if new_state {
                let _ = remove_user_state();
            }
            return Err(error.into());
        }
    };

    if let Err(error) = apply_color_scheme(&scheme_id, Some(&palette.accent)) {
        if let Some(theme) = previous_theme.as_deref() {
            let _ = activate_desktop_theme(theme);
        }
        let _ = fs::remove_file(&scheme_path);
        let _ = fs::remove_dir_all(generated_theme_dir(&generated_theme_id)?);
        if new_state {
            let _ = remove_user_state();
        }
        return Err(error.into());
    }
    notify_appearance_changes(&scheme_id, &palette.accent, &generated_theme_id)?;
    apply_inactive_header_colors(palette)?;
    verify_active_scheme_colors(&scheme_id)?;
    verify_active_inactive_header_colors(&scheme_id)?;
    remove_obsolete_generated_themes(Some(&generated_theme_id))?;

    println!(
        "{}",
        serde_json::to_string(&ApplyResult {
            action: "apply",
            name: display_name,
        })?
    );
    Ok(())
}

fn get_or_create_state() -> io::Result<(UserState, bool)> {
    if let Some(state) = read_user_state()? {
        return Ok((state, false));
    }

    let current_scheme = active_color_scheme()?;
    let previous_color_scheme = current_scheme
        .filter(|scheme| !is_widget_scheme_id(scheme))
        .or_else(|| Some("BreezeDark".to_string()));
    Ok((
        UserState {
            previous_color_scheme,
            previous_desktop_theme: active_desktop_theme()?,
            previous_accent_color: active_accent_color()?,
            previous_inactive_header_background: inactive_header_color("BackgroundNormal")?,
            previous_inactive_header_alternate: inactive_header_color("BackgroundAlternate")?,
            appearance_settings_captured: true,
        },
        true,
    ))
}

fn install_generated_desktop_theme(
    state: &UserState,
    palette: &Palette,
    scheme_id: &str,
) -> io::Result<String> {
    let source_id = state
        .previous_desktop_theme
        .as_deref()
        .ok_or_else(|| io::Error::other("No active Plasma desktop theme could be determined"))?;
    if is_generated_theme_id(source_id) {
        return Err(io::Error::other(
            "The original Plasma desktop theme is not available",
        ));
    }
    let source = find_desktop_theme(source_id)?;
    let data_home = user_data_dir()?;
    let themes_dir = data_home.join("plasma/desktoptheme");
    fs::create_dir_all(&themes_dir)?;
    let theme_id = generated_theme_id_for_scheme(scheme_id);
    let destination = themes_dir.join(&theme_id);
    if destination.exists() {
        verify_generated_theme(&destination)?;
        fs::remove_dir_all(&destination)?;
    }

    copy_directory(&source, &destination)?;
    let previous_theme = active_desktop_theme()?;
    let result = update_generated_theme_files(&destination, palette, &theme_id)
        .and_then(|()| activate_desktop_theme(&theme_id));
    if let Err(error) = result {
        if let Some(previous) = previous_theme.as_deref() {
            let _ = activate_desktop_theme(previous);
        }
        let _ = fs::remove_dir_all(&destination);
        return Err(error);
    }
    Ok(theme_id)
}

fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "Unsupported file in Plasma theme: {}",
                    entry.path().display()
                ),
            ));
        }
    }
    Ok(())
}

fn update_generated_theme_files(
    theme_dir: &Path,
    palette: &Palette,
    theme_id: &str,
) -> io::Result<()> {
    let metadata_path = theme_dir.join("metadata.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata_path)?).map_err(io::Error::other)?;
    let plugin = metadata
        .get_mut("KPlugin")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid Plasma theme metadata")
        })?;
    plugin.insert(
        "Id".to_string(),
        serde_json::Value::String(theme_id.to_string()),
    );
    plugin.insert(
        "Name".to_string(),
        serde_json::Value::String("PlasmaColorPicker Theme".to_string()),
    );
    plugin.insert(
        "Description".to_string(),
        serde_json::Value::String("Generated by PlasmaColorPicker".to_string()),
    );
    metadata["X-WallpaperPalette-Owner"] =
        serde_json::Value::String(WIDGET_OWNER_MARKER.to_string());
    fs::write(
        metadata_path,
        serde_json::to_vec_pretty(&metadata).map_err(io::Error::other)?,
    )?;

    let panel_path = theme_dir.join("widgets/panel-background.svg");
    if panel_path.is_file() {
        let svg = fs::read_to_string(&panel_path)?;
        fs::write(
            panel_path,
            recolor_panel_svg(&svg, &palette.panel, &palette.background)?,
        )?;
    }
    fs::write(theme_dir.join("colors"), plasma_theme_colors(palette))?;
    Ok(())
}

fn recolor_panel_svg(svg: &str, top_color: &str, bottom_color: &str) -> io::Result<String> {
    let mut result = String::with_capacity(svg.len());
    let mut remaining = svg;
    let mut index = 0;
    while let Some(marker_index) = remaining.find("stop-color:") {
        let after_marker = marker_index + "stop-color:".len();
        result.push_str(&remaining[..after_marker]);
        result.push_str(if index == 0 { top_color } else { bottom_color });
        let after_color = remaining[after_marker..]
            .find(|character: char| !character.is_ascii_hexdigit() && character != '#')
            .map(|offset| after_marker + offset)
            .unwrap_or(remaining.len());
        remaining = &remaining[after_color..];
        index += 1;
    }
    result.push_str(remaining);
    if index == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The Plasma panel SVG has no editable gradient stops",
        ));
    }
    Ok(result)
}

fn plasma_theme_colors(palette: &Palette) -> String {
    let panel = kde_rgb(&palette.panel);
    let background = kde_rgb(&palette.background);
    let surface = kde_rgb(&palette.surface);
    let accent = kde_rgb(&palette.accent);
    let selection = kde_rgb(&palette.selection);
    let selection_foreground = kde_rgb(&palette.selection_foreground);
    let foreground = "244,244,245";
    let inactive = "170,170,175";
    format!(
        "[General]\nName=PlasmaColorPicker Theme\nVersion=1.0\nX-WallpaperPalette-Owner={WIDGET_OWNER_MARKER}\n\n\
         [Background:Normal]\nColor={background}\nTotalColor={background}\n\n\
         [Background:Window]\nColor={background}\nTotalColor={background}\n\n\
         [Foreground:Normal]\nColor={foreground}\n\n\
         [Foreground:Link]\nColor={accent}\n\n\
         [Foreground:Active]\nColor={accent}\n\n\
         [Foreground:Inactive]\nColor={inactive}\n\n\
         [Colors:Button]\nBackgroundNormal={surface}\nBackgroundAlternate={background}\nForegroundNormal={foreground}\nForegroundActive={accent}\nHoverColor={selection}\nFocusColor={accent}\n\n\
         [Colors:Selection]\nBackgroundNormal={selection}\nBackgroundAlternate={selection}\nForegroundNormal={selection_foreground}\nForegroundActive={selection_foreground}\n\n\
         [Colors:View]\nBackgroundNormal={background}\nBackgroundAlternate={surface}\nForegroundNormal={foreground}\nHoverColor={selection}\nFocusColor={accent}\n\n\
         [Colors:Window]\nBackgroundNormal={background}\nBackgroundAlternate={surface}\nForegroundNormal={foreground}\n\n\
         [Colors:Header]\nBackgroundNormal={panel}\nBackgroundAlternate={background}\nForegroundNormal={foreground}\nForegroundActive={accent}\n\n\
         [Colors:Complementary]\nBackgroundNormal={background}\nBackgroundAlternate={surface}\nForegroundNormal={foreground}\nForegroundActive={accent}\n\n\
         [Colors:Tooltip]\nBackgroundNormal={background}\nBackgroundAlternate={surface}\nForegroundNormal={foreground}\n",
    )
}

fn kde_rgb(color: &str) -> String {
    let value = u32::from_str_radix(&color[1..], 16).expect("palette colors are hexadecimal");
    format!(
        "{},{},{}",
        (value >> 16) & 0xff,
        (value >> 8) & 0xff,
        value & 0xff
    )
}

fn mix_hex(left: &str, right: &str, right_weight: f64) -> String {
    let left = u32::from_str_radix(&left[1..], 16).expect("palette colors are hexadecimal");
    let right = u32::from_str_radix(&right[1..], 16).expect("palette colors are hexadecimal");
    let channel = |shift: u32| {
        let left = ((left >> shift) & 0xff) as f64;
        let right = ((right >> shift) & 0xff) as f64;
        (left * (1.0 - right_weight) + right * right_weight).round() as u8
    };
    format!("#{:02X}{:02X}{:02X}", channel(16), channel(8), channel(0))
}

fn activate_desktop_theme(theme_id: &str) -> io::Result<()> {
    let output = Command::new("plasma-apply-desktoptheme")
        .arg(theme_id)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "KDE could not apply the Plasma theme: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn verify_generated_theme(path: &Path) -> io::Result<()> {
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("metadata.json"))?).map_err(io::Error::other)?;
    let owner = metadata
        .get("X-WallpaperPalette-Owner")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if owner == WIDGET_OWNER_MARKER || owner == LEGACY_WIDGET_OWNER_MARKER {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "Refusing to overwrite an unowned theme at {}",
                path.display()
            ),
        ))
    }
}

// Restore prior appearance settings and remove only themes/schemes owned by PCP.
fn remove_user_state() -> io::Result<()> {
    let path = user_state_path()?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn is_widget_scheme_id(id: &str) -> bool {
    legacy_scheme_index(id).is_some()
}

fn legacy_scheme_index(id: &str) -> Option<usize> {
    let (base, suffix) = id.split_once('-').unwrap_or((id, ""));
    if !suffix.is_empty() && !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    match base {
        "PlasmaColorPicker01" => Some(0),
        "PlasmaColorPicker02" => Some(1),
        "PlasmaColorPicker03" => Some(2),
        "WallpaperPalette01" => Some(0),
        "WallpaperPalette02" => Some(1),
        "WallpaperPalette03" => Some(2),
        _ => None,
    }
}

fn remove_owned_palettes() -> Result<RemoveResult, Box<dyn std::error::Error>> {
    let colors_dir = user_colors_dir()?;
    let mut owned_files = Vec::new();
    if colors_dir.is_dir() {
        for entry in fs::read_dir(&colors_dir)? {
            let path = entry?.path();
            if is_owned_scheme_file(&path) {
                owned_files.push(path);
            }
        }
    }
    let state = read_user_state()?;
    let active_scheme = active_color_scheme()?.unwrap_or_default();
    let active_theme = active_desktop_theme()?.unwrap_or_default();
    let active_widget_appearance = is_generated_theme_id(&active_theme)
        || owned_files.iter().any(|path| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .is_some_and(|id| id == active_scheme)
        });

    if owned_files.iter().any(|path| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|id| id == active_scheme)
    }) {
        let fallback = state
            .as_ref()
            .and_then(|state| state.previous_color_scheme.as_deref())
            .filter(|scheme| !is_widget_scheme_id(scheme))
            .map(str::to_string)
            .unwrap_or_else(|| "BreezeDark".to_string());
        apply_color_scheme(&fallback, None)?;
    }

    if is_generated_theme_id(&active_theme) {
        let original_theme = state
            .as_ref()
            .and_then(|state| state.previous_desktop_theme.as_deref())
            .ok_or_else(|| io::Error::other("Cannot restore the previous Plasma theme"))?;
        activate_desktop_theme(original_theme)?;
    }
    if active_widget_appearance {
        restore_appearance_settings(state.as_ref(), &owned_files)?;
    }
    let removed_theme = remove_obsolete_generated_themes(None)?;

    let mut removed_schemes = 0;
    let mut failures = Vec::new();
    for path in owned_files {
        match fs::remove_file(&path) {
            Ok(()) => removed_schemes += 1,
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "Some palettes could not be removed: {}",
            failures.join("; ")
        )
        .into());
    }
    remove_user_state()?;
    Ok(RemoveResult {
        action: "remove",
        removed_schemes,
        removed_theme,
    })
}

fn generated_theme_dir(theme_id: &str) -> io::Result<PathBuf> {
    Ok(user_data_dir()?.join("plasma/desktoptheme").join(theme_id))
}

fn remove_obsolete_generated_themes(active_theme: Option<&str>) -> io::Result<bool> {
    let themes_dir = user_data_dir()?.join("plasma/desktoptheme");
    if !themes_dir.is_dir() {
        return Ok(false);
    }

    let mut removed = false;
    for entry in fs::read_dir(themes_dir)? {
        let path = entry?.path();
        let Some(theme_id) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !path.is_dir() || !is_generated_theme_id(theme_id) || active_theme == Some(theme_id) {
            continue;
        }
        verify_generated_theme(&path)?;
        fs::remove_dir_all(path)?;
        removed = true;
    }
    Ok(removed)
}

// Read, create, and apply KDE color-scheme files; includes color conversion helpers.
fn is_owned_scheme_file(path: &Path) -> bool {
    if path.extension().and_then(|ext| ext.to_str()) != Some("colors") {
        return false;
    }
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    let owner = read_ini_value(&contents, "General", "X-WallpaperPalette-Owner");
    if owner.as_deref() == Some(WIDGET_OWNER_MARKER)
        || owner.as_deref() == Some(LEGACY_WIDGET_OWNER_MARKER)
    {
        return true;
    }
    let Some(index) = legacy_scheme_index(stem) else {
        return false;
    };
    let expected = [
        format!("PlasmaColorPicker {:02}", index + 1),
        format!("Wallpaper Palette {:02}", index + 1),
    ];
    read_ini_value(&contents, "General", "Name").is_some_and(|name| {
        expected
            .iter()
            .any(|expected| name == *expected || name.starts_with(&format!("{expected} (")))
    })
}

fn read_ini_value(contents: &str, group: &str, key: &str) -> Option<String> {
    let target_group = format!("[{group}]");
    let mut in_group = false;
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            in_group = line == target_group;
        } else if in_group {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim() == key {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

fn apply_color_scheme(scheme_id: &str, accent: Option<&str>) -> io::Result<()> {
    let previous_accent = active_accent_color()?;
    if accent.is_some() {
        delete_kde_setting("kdeglobals", "General", "AccentColor")?;
    }

    let output = match Command::new("plasma-apply-colorscheme")
        .arg(scheme_id)
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            restore_previous_accent(previous_accent.as_deref())?;
            return Err(error);
        }
    };
    if !output.status.success() {
        let message = format!(
            "KDE could not apply color scheme {scheme_id}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        restore_previous_accent(previous_accent.as_deref())?;
        return Err(io::Error::other(message));
    }

    let active_scheme = match active_color_scheme() {
        Ok(scheme) => scheme.unwrap_or_default(),
        Err(error) => {
            restore_previous_accent(previous_accent.as_deref())?;
            return Err(error);
        }
    };
    if active_scheme != scheme_id {
        restore_previous_accent(previous_accent.as_deref())?;
        return Err(io::Error::other(format!(
            "KDE reported success, but the active color scheme is {active_scheme}"
        )));
    }

    if let Some(accent) = accent {
        if let Err(error) =
            notify_kde_setting("kdeglobals", "General", "AccentColor", &kde_rgb(accent))
        {
            restore_previous_accent(previous_accent.as_deref())?;
            return Err(error);
        }
        let expected = kde_rgb(accent);
        let active_accent = match active_accent_color() {
            Ok(accent) => accent.unwrap_or_default(),
            Err(error) => {
                restore_previous_accent(previous_accent.as_deref())?;
                return Err(error);
            }
        };
        if active_accent != expected {
            restore_previous_accent(previous_accent.as_deref())?;
            return Err(io::Error::other(format!(
                "KDE reported success, but the active accent color is {active_accent}"
            )));
        }
    }
    Ok(())
}

fn restore_previous_accent(accent: Option<&str>) -> io::Result<()> {
    if let Some(accent) = accent {
        notify_kde_setting("kdeglobals", "General", "AccentColor", accent)?;
    }
    Ok(())
}

fn unique_scheme_id(colors_dir: &Path, index: usize) -> Result<String, io::Error> {
    let base = format!("PlasmaColorPicker{:02}", index + 1);
    if !colors_dir.join(format!("{base}.colors")).exists() {
        return Ok(base);
    }
    for suffix in 2..=9999 {
        let candidate = format!("{base}-{suffix}");
        if !colors_dir.join(format!("{candidate}.colors")).exists() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Too many saved wallpaper palettes with this name",
    ))
}

fn scheme_display_name(scheme_id: &str, index: usize) -> String {
    let base = format!("PlasmaColorPicker{:02}", index + 1);
    if scheme_id.starts_with(&format!("WallpaperPalette{:02}", index + 1)) {
        let legacy_base = format!("WallpaperPalette{:02}", index + 1);
        let suffix = scheme_id.strip_prefix(&legacy_base).unwrap_or_default();
        return if let Some(number) = suffix.strip_prefix('-') {
            format!("Wallpaper Palette {:02} ({number})", index + 1)
        } else {
            format!("Wallpaper Palette {:02}", index + 1)
        };
    }
    let suffix = scheme_id.strip_prefix(&base).unwrap_or_default();
    if let Some(number) = suffix.strip_prefix('-') {
        format!("PlasmaColorPicker {:02} ({number})", index + 1)
    } else {
        format!("PlasmaColorPicker {:02}", index + 1)
    }
}

fn colors_file(name: &str, palette: &Palette) -> String {
    let mut output = format!(
        "[General]\nName={name}\nColorScheme={name}\nshadeSortColumn=true\nX-WallpaperPalette-Owner={WIDGET_OWNER_MARKER}\n\n\
         [ColorEffects:Disabled]\nColor=112,111,110\nColorAmount=0\nColorEffect=0\nContrastAmount=0.65\nContrastEffect=1\nIntensityAmount=0.1\nIntensityEffect=2\n\n\
         [ColorEffects:Inactive]\nChangeSelectionColor=false\nColor=112,111,110\nColorAmount=0.025\nColorEffect=2\nContrastAmount=0.1\nContrastEffect=2\nEnable=true\nIntensityAmount=0\nIntensityEffect=0\n\n"
    );
    for group in [
        "Button",
        "Complementary",
        "Header",
        "Tooltip",
        "View",
        "Window",
    ] {
        output.push_str(&format!("[Colors:{group}]\n"));
        let group_background = match group {
            "Header" => &palette.panel,
            "Button" => &palette.surface,
            _ => &palette.background,
        };
        output.push_str(&format!("BackgroundNormal={}\n", kde_rgb(group_background)));
        output.push_str(&format!(
            "BackgroundAlternate={}\n",
            kde_rgb(&palette.surface)
        ));
        output.push_str(&format!(
            "ForegroundNormal={}\n",
            kde_rgb(&palette.foreground)
        ));
        output.push_str(&format!("ForegroundInactive=170,170,175\n"));
        output.push_str(&format!("ForegroundActive={}\n", kde_rgb(&palette.accent)));
        output.push_str(&format!("ForegroundLink={}\n", kde_rgb(&palette.accent)));
        output.push_str(&format!("ForegroundVisited={}\n", kde_rgb(&palette.accent)));
        output.push_str("ForegroundNegative=218,68,83\n");
        output.push_str("ForegroundNeutral=246,166,0\n");
        output.push_str("ForegroundPositive=39,174,96\n");
        output.push_str(&format!("DecorationFocus={}\n", kde_rgb(&palette.accent)));
        output.push_str(&format!(
            "DecorationHover={}\n\n",
            kde_rgb(&palette.selection)
        ));
    }
    output.push_str("[Colors:Header][Inactive]\n");
    output.push_str(&format!(
        "BackgroundNormal={}\nBackgroundAlternate={}\n",
        kde_rgb(&palette.panel),
        kde_rgb(&palette.background)
    ));
    output.push_str(&format!(
        "ForegroundNormal={}\nForegroundInactive=170,170,175\nForegroundActive={}\nForegroundLink={}\nForegroundVisited={}\nForegroundNegative=218,68,83\nForegroundNeutral=246,166,0\nForegroundPositive=39,174,96\nDecorationFocus={}\nDecorationHover={}\n\n",
        kde_rgb(&palette.foreground),
        kde_rgb(&palette.accent),
        kde_rgb(&palette.accent),
        kde_rgb(&palette.accent),
        kde_rgb(&palette.accent),
        kde_rgb(&palette.selection)
    ));
    output.push_str(&format!(
        "[Colors:Selection]\nBackgroundNormal={}\nBackgroundAlternate={}\nForegroundNormal={}\nForegroundInactive=230,230,230\nForegroundActive={}\nForegroundLink={}\nForegroundVisited={}\nForegroundNegative=218,68,83\nForegroundNeutral=246,166,0\nForegroundPositive=39,174,96\nDecorationFocus={}\nDecorationHover={}\n\n",
        kde_rgb(&palette.selection),
        kde_rgb(&mix_hex(&palette.selection, &palette.background, 0.12)),
        kde_rgb(&palette.selection_foreground),
        kde_rgb(&palette.selection_foreground),
        kde_rgb(&palette.accent),
        kde_rgb(&palette.accent),
        kde_rgb(&palette.accent),
        kde_rgb(&palette.selection)
    ));
    output.push_str(&format!(
        "[WM]\nactiveBackground={}\nactiveForeground={}\ninactiveBackground={}\ninactiveForeground={}\n",
        kde_rgb(&palette.background),
        kde_rgb(&palette.foreground),
        kde_rgb(&palette.background),
        kde_rgb(&palette.foreground)
    ));
    output
}

fn hex(rgb: (f64, f64, f64)) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        (rgb.0 * 255.0).round() as u8,
        (rgb.1 * 255.0).round() as u8,
        (rgb.2 * 255.0).round() as u8
    )
}

fn contrast_foreground(rgb: (f64, f64, f64)) -> String {
    fn luminance_component(component: f64) -> f64 {
        if component <= 0.04045 {
            component / 12.92
        } else {
            ((component + 0.055) / 1.055).powf(2.4)
        }
    }

    let luminance = 0.2126 * luminance_component(rgb.0)
        + 0.7152 * luminance_component(rgb.1)
        + 0.0722 * luminance_component(rgb.2);
    if 1.05 / (luminance + 0.05) >= (luminance + 0.05) / 0.05 {
        "#FFFFFF".to_string()
    } else {
        "#000000".to_string()
    }
}

fn rgb_to_hsl(red: f64, green: f64, blue: f64) -> (f64, f64, f64) {
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, lightness);
    }
    let delta = max - min;
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == red {
        ((green - blue) / delta).rem_euclid(6.0)
    } else if max == green {
        (blue - red) / delta + 2.0
    } else {
        (red - green) / delta + 4.0
    } * 60.0;
    (hue, saturation, lightness)
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> (f64, f64, f64) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let match_value = lightness - chroma / 2.0;
    let (red, green, blue) = match hue as u32 {
        0..=59 => (chroma, x, 0.0),
        60..=119 => (x, chroma, 0.0),
        120..=179 => (0.0, chroma, x),
        180..=239 => (0.0, x, chroma),
        240..=299 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    (red + match_value, green + match_value, blue + match_value)
}

#[cfg(test)]
mod tests;

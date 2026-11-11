use image::{ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
            let wallpaper = current_wallpaper()?;
            let palettes = create_palettes(&wallpaper)?;
            println!(
                "{}",
                serde_json::to_string(&AnalyzeResult {
                    action: "analyze",
                    palettes,
                })?
            );
        }
        Some(action @ ("icons" | "cursors")) => {
            let index = args
                .next()
                .ok_or_else(|| format!("Usage: plasmacolorpicker-helper {action} <0|1|2>"))?
                .parse::<usize>()?;
            if index > 2 || args.next().is_some() {
                return Err("Palette index must be 0, 1, or 2".into());
            }
            let wallpaper = current_wallpaper()?;
            let palettes = create_palettes(&wallpaper)?;
            let search_color = hue_search_term(&palettes[index].accent)?;
            let (category, action) = if action == "icons" {
                (132, "icons")
            } else {
                (107, "cursors")
            };
            let themes = search_store_themes(&search_color, category)?;
            println!(
                "{}",
                serde_json::to_string(&StoreThemesResult {
                    action,
                    search_color,
                    themes,
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
            if index > 2 || args.next().is_some() {
                return Err("Palette index must be 0, 1, or 2".into());
            }
            let wallpaper = current_wallpaper()?;
            let palettes = create_palettes(&wallpaper)?;
            apply_palette(&palettes[index])?;
        }
        _ => {
            return Err(
                "Usage: plasmacolorpicker-helper analyze | icons INDEX | cursors INDEX | install-theme <icons|cursors> <archive> | diagnose | apply INDEX | remove".into(),
            )
        }
    }
    Ok(())
}

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
    if response.get("status").and_then(serde_json::Value::as_str) != Some("ok") {
        return Err("KDE Store did not return a successful catalog response".into());
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

fn current_wallpaper() -> Result<PathBuf, Box<dyn std::error::Error>> {
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

    let mut current_screen = None;
    let mut candidates = Vec::new();
    let mut active_image_section = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            active_image_section = line.contains("][Wallpaper][org.kde.image][General]");
            if line.starts_with("[Containments][") && !line.contains("][Wallpaper]") {
                current_screen = None;
            }
            continue;
        }

        if line.starts_with("lastScreen=") {
            current_screen = line["lastScreen=".len()..].parse::<i32>().ok();
        } else if active_image_section && line.starts_with("Image=") {
            let value = line["Image=".len()..].trim();
            if let Some(path) = local_file_path(value) {
                candidates.push((current_screen.unwrap_or(-1), path));
            }
        }
    }

    candidates
        .iter()
        .find(|(screen, path)| *screen == 0 && path.is_file())
        .or_else(|| candidates.iter().find(|(_, path)| path.is_file()))
        .map(|(_, path)| path.clone())
        .ok_or_else(|| {
            format!(
                "No readable image wallpaper was found in {}",
                config_path.display()
            )
            .into()
        })
}

fn local_file_path(value: &str) -> Option<PathBuf> {
    let value = value.strip_prefix("file://")?;
    let decoded = percent_decode(value)?;
    Some(PathBuf::from(decoded))
}

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

    let image_check = wallpaper.and_then(|path| {
        ImageReader::open(&path)
            .map_err(|error| error.to_string())?
            .with_guessed_format()
            .map_err(|error| error.to_string())?
            .decode()
            .map(|_| path)
            .map_err(|error| error.to_string())
    });
    checks.push(DiagnosticCheck {
        id: "image",
        ok: image_check.is_ok(),
        details: image_check
            .map(|path| format!("Decoded {}", path.display()))
            .unwrap_or_else(|error| error),
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

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn create(parent: &Path, prefix: &str) -> io::Result<Self> {
        for attempt in 0..100 {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = parent.join(format!("{prefix}-{}-{nonce}-{attempt}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Could not create a unique temporary directory",
        ))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn install_theme(
    theme_type: &str,
    archive_argument: &str,
) -> Result<InstallThemeResult, Box<dyn std::error::Error>> {
    if !matches!(theme_type, "icons" | "cursors") {
        return Err("Theme type must be icons or cursors".into());
    }
    let archive_path = local_file_path(archive_argument)
        .or_else(|| {
            let path = PathBuf::from(archive_argument);
            path.is_absolute().then_some(path)
        })
        .ok_or("Select a local archive file")?;
    if !archive_path.is_absolute() {
        return Err("The selected archive path must be local and absolute".into());
    }
    let metadata = fs::metadata(&archive_path)?;
    if !metadata.is_file() {
        return Err("The selected path is not a regular file".into());
    }
    if metadata.len() == 0 || metadata.len() > 512 * 1024 * 1024 {
        return Err("The archive must be non-empty and no larger than 512 MiB".into());
    }
    let unar = find_executable("unar");
    let seven_zip = find_executable("7z");
    if unar.is_none() && seven_zip.is_none() {
        return Err(
            "Neither unar nor 7z is installed; install one of them or use System Settings to install this theme manually"
                .into(),
        );
    }

    let staging = TemporaryDirectory::create(&env::temp_dir(), "pcp-theme-archive")?;
    extract_theme_archive(
        unar.as_deref(),
        seven_zip.as_deref(),
        &archive_path,
        theme_type,
        &staging.0,
    )?;

    let mut candidates = Vec::new();
    let mut entry_count = 0usize;
    let mut extracted_bytes = 0u64;
    let staging_root = fs::canonicalize(&staging.0)?;
    validate_extracted_tree(
        &staging_root,
        &staging_root,
        0,
        &mut entry_count,
        &mut extracted_bytes,
    )?;
    find_theme_directories(&staging.0, theme_type, 0, &mut candidates)?;
    if candidates.is_empty() {
        return Err(format!(
            "No installable {theme_type} theme was found: the archive must contain an index.theme file and theme assets. It may be an add-on pack rather than the main theme."
        )
        .into());
    }

    let install_directory = theme_install_directory(theme_type)?;
    fs::create_dir_all(&install_directory)?;
    let existing_theme_directories = icon_theme_directories()?;
    let mut reserved_folder_names = Vec::with_capacity(candidates.len());
    let mut themes_to_install = Vec::with_capacity(candidates.len());
    for theme_root in candidates {
        let index_contents = fs::read_to_string(theme_root.join("index.theme"))?;
        let display_name = read_ini_value(&index_contents, "Icon Theme", "Name")
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| {
                theme_root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into()
            });
        let original_folder_name = if theme_root == staging.0 {
            archive_theme_id(&archive_path)
        } else {
            safe_theme_id(&theme_root.file_name().unwrap_or_default().to_string_lossy())
        }
        .ok_or("The theme folder name is not valid")?;
        let folder_name = unique_theme_folder_name(
            &original_folder_name,
            &existing_theme_directories,
            &reserved_folder_names,
        )?;
        reserved_folder_names.push(folder_name.clone());
        themes_to_install.push((display_name, original_folder_name, folder_name, theme_root));
    }

    let install_staging = TemporaryDirectory::create(&install_directory, ".pcp-theme-install")?;
    let renamed_theme_ids = themes_to_install
        .iter()
        .filter(|(_, original, folder, _)| original != folder)
        .map(|(_, original, folder, _)| (original.clone(), folder.clone()))
        .collect::<Vec<_>>();
    for (_, original_folder_name, folder_name, theme_root) in &themes_to_install {
        let canonical_theme_root = fs::canonicalize(theme_root)?;
        let staged_theme = install_staging.0.join(folder_name);
        copy_theme_tree(theme_root, &staged_theme, &canonical_theme_root)?;
        let make_single_hidden_theme_visible = themes_to_install.len() == 1;
        if make_single_hidden_theme_visible {
            make_hidden_theme_selectable(&staged_theme, folder_name)?;
        }
        rewrite_theme_index_for_install(
            &staged_theme,
            original_folder_name,
            folder_name,
            &renamed_theme_ids,
        )?;
    }

    let mut installed_paths = Vec::with_capacity(themes_to_install.len());
    let mut installed = Vec::with_capacity(themes_to_install.len());
    for (display_name, _, folder_name, _) in themes_to_install {
        let staged_theme = install_staging.0.join(&folder_name);
        let destination = install_directory.join(&folder_name);
        if let Err(error) = fs::create_dir(&destination) {
            for path in &installed_paths {
                let _ = fs::remove_dir_all(path);
            }
            return Err(error.into());
        }
        installed_paths.push(destination.clone());
        let move_result = fs::read_dir(&staged_theme).and_then(|entries| {
            for entry in entries {
                let entry = entry?;
                fs::rename(entry.path(), destination.join(entry.file_name()))?;
            }
            Ok(())
        });
        if let Err(error) = move_result {
            for path in &installed_paths {
                let _ = fs::remove_dir_all(path);
            }
            return Err(error.into());
        }
        fs::remove_dir(staged_theme)?;
        let installed_index = fs::read_to_string(destination.join("index.theme"))?;
        let display_name = read_ini_value(&installed_index, "Icon Theme", "Name")
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(display_name);
        installed.push(InstalledTheme {
            name: display_name,
            install_path: destination.display().to_string(),
        });
    }

    Ok(InstallThemeResult {
        action: "install-theme",
        theme_type: theme_type.to_string(),
        themes: installed,
    })
}

fn extract_theme_archive(
    unar: Option<&Path>,
    seven_zip: Option<&Path>,
    archive: &Path,
    theme_type: &str,
    destination: &Path,
) -> io::Result<()> {
    let extract = |input: &Path, output: &Path| {
        if let Some(unar) = unar {
            extract_with_unar(unar, input, output)
        } else {
            let seven_zip = seven_zip.ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "Neither unar nor 7z is available")
            })?;
            extract_with_7z_checked(seven_zip, input, output)
        }
    };
    extract(archive, destination)?;
    for _ in 0..4 {
        let mut themes = Vec::new();
        find_theme_directories(destination, theme_type, 0, &mut themes)?;
        if !themes.is_empty() {
            return Ok(());
        }
        let Some(inner_tar) = find_inner_tar(destination)? else {
            return Ok(());
        };
        let nested_layer = TemporaryDirectory::create(destination, ".pcp-tar-layer")?;
        extract(&inner_tar, &nested_layer.0)?;
        fs::remove_file(inner_tar)?;
        for entry in fs::read_dir(&nested_layer.0)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            match fs::symlink_metadata(&target) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::rename(entry.path(), target)?;
                }
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "Nested archive layers contain conflicting path {}",
                            target.display()
                        ),
                    ));
                }
                Err(error) => return Err(error),
            }
        }
    }
    if find_inner_tar(destination)?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The archive contains too many nested TAR layers",
        ));
    }
    Ok(())
}

fn extract_with_7z_checked(seven_zip: &Path, archive: &Path, destination: &Path) -> io::Result<()> {
    if let Err(error) = extract_with_7z(seven_zip, archive, destination) {
        let details = error.to_string();
        if details.contains("Dangerous link path") {
            return Err(io::Error::other(format!(
                "{details}. This archive uses relative symbolic links; install unar to preserve them safely"
            )));
        }
        return Err(error);
    }
    Ok(())
}

fn find_inner_tar(directory: &Path) -> io::Result<Option<PathBuf>> {
    let mut tar_files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && is_tar_archive(&entry.path())? {
            tar_files.push(entry.path());
        }
    }
    if tar_files.len() > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The compressed archive produced more than one top-level TAR file",
        ));
    }
    Ok(tar_files.pop())
}

fn is_tar_archive(path: &Path) -> io::Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut header = [0u8; 512];
    match io::Read::read_exact(&mut file, &mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
        Err(error) => return Err(error),
    }
    let checksum_field = std::str::from_utf8(&header[148..156]).ok();
    let stored_checksum = checksum_field
        .map(|field| {
            field.trim_matches(|character: char| {
                character == '\0' || character.is_ascii_whitespace()
            })
        })
        .and_then(|field| u64::from_str_radix(field, 8).ok());
    let calculated_checksum = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                u64::from(b' ')
            } else {
                u64::from(*byte)
            }
        })
        .sum::<u64>();
    Ok(stored_checksum == Some(calculated_checksum))
}

fn extract_with_unar(unar: &Path, archive: &Path, destination: &Path) -> io::Result<()> {
    let output = Command::new(unar)
        .args(["-q", "-nr", "-o"])
        .arg(destination)
        .arg(archive)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format_extraction_error(
        "unar",
        &output.stderr,
    )))
}

fn extract_with_7z(seven_zip: &Path, archive: &Path, destination: &Path) -> io::Result<()> {
    let output = Command::new(seven_zip)
        .arg("x")
        .arg("-y")
        .arg(format!("-o{}", destination.display()))
        .arg(archive)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format_extraction_error(
        "7z",
        &output.stderr,
    )))
}

fn format_extraction_error(extractor: &str, stderr: &[u8]) -> String {
    let details = String::from_utf8_lossy(stderr);
    let lines = details
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(8)
        .collect::<Vec<_>>();
    let summary = if lines.is_empty() {
        "no details were provided".to_string()
    } else {
        lines.join("\n")
    };
    let omitted = details
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let suffix = if omitted > lines.len() {
        format!("\n… and {} more lines", omitted - lines.len())
    } else {
        String::new()
    };
    format!("{extractor} could not extract the archive: {summary}{suffix}")
}

fn icon_theme_directories() -> io::Result<Vec<PathBuf>> {
    let mut directories = vec![user_data_dir()?.join("icons")];
    if let Some(home) = env::var_os("HOME") {
        directories.push(PathBuf::from(home).join(".icons"));
    }
    let data_dirs = env::var_os("XDG_DATA_DIRS")
        .as_deref()
        .map(env::split_paths)
        .map(Iterator::collect)
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    directories.extend(
        data_dirs
            .into_iter()
            .map(|directory| directory.join("icons")),
    );
    directories.sort();
    directories.dedup();
    Ok(directories)
}

fn theme_install_directory(theme_type: &str) -> io::Result<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Cannot determine home directory")
    })?;
    let data_home = user_data_dir()?;
    theme_install_directory_for(theme_type, &data_home, &home)
}

fn theme_install_directory_for(
    theme_type: &str,
    data_home: &Path,
    home: &Path,
) -> io::Result<PathBuf> {
    match theme_type {
        "icons" => Ok(data_home.join("icons")),
        "cursors" => Ok(home.join(".icons")),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Theme type must be icons or cursors",
        )),
    }
}

fn unique_theme_folder_name(
    requested: &str,
    theme_directories: &[PathBuf],
    reserved: &[String],
) -> io::Result<String> {
    let is_available = |name: &str| -> io::Result<bool> {
        if reserved.iter().any(|reserved_name| reserved_name == name) {
            return Ok(false);
        }
        for directory in theme_directories {
            match fs::symlink_metadata(directory.join(name)) {
                Ok(_) => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    };
    if is_available(requested)? {
        return Ok(requested.to_string());
    }
    for suffix in 1..=999 {
        let suffix = if suffix == 1 {
            "-PCP".to_string()
        } else {
            format!("-PCP-{suffix}")
        };
        let base_length = 128usize.saturating_sub(suffix.len());
        let base = requested.chars().take(base_length).collect::<String>();
        let candidate = format!("{}{}", base.trim_end_matches(['-', '.', '_']), suffix);
        if is_available(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("Could not find a conflict-free folder name for {requested}"),
    ))
}

fn rewrite_theme_index_for_install(
    theme_root: &Path,
    original_folder_name: &str,
    folder_name: &str,
    renamed_theme_ids: &[(String, String)],
) -> io::Result<()> {
    let index_path = theme_root.join("index.theme");
    let contents = fs::read_to_string(&index_path)?;
    let renamed = original_folder_name != folder_name;
    let mut current_group = String::new();
    let mut rewritten = String::with_capacity(contents.len());
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_group = trimmed[1..trimmed.len() - 1].to_string();
            rewritten.push_str(line);
            rewritten.push('\n');
            continue;
        }
        if current_group == "Icon Theme" {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                if key == "Name" && renamed {
                    rewritten.push_str(&format!("{key}={} (PCP)\n", value.trim()));
                    continue;
                }
                if key == "Inherits" {
                    let inherits = value
                        .split(',')
                        .map(str::trim)
                        .map(|inherit| {
                            renamed_theme_id(inherit, renamed_theme_ids)
                                .unwrap_or(inherit)
                                .to_string()
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    rewritten.push_str(&format!("{key}={inherits}\n"));
                    continue;
                }
            }
        }
        rewritten.push_str(line);
        rewritten.push('\n');
    }
    if rewritten != contents {
        fs::write(index_path, rewritten)?;
    }
    Ok(())
}

fn renamed_theme_id<'a>(theme_id: &'a str, renamed: &'a [(String, String)]) -> Option<&'a str> {
    renamed
        .iter()
        .find(|(original, _)| original == theme_id)
        .map(|(_, replacement)| replacement.as_str())
}

fn validate_extracted_tree(
    archive_root: &Path,
    directory: &Path,
    depth: usize,
    entry_count: &mut usize,
    extracted_bytes: &mut u64,
) -> io::Result<()> {
    if depth > 12 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The archive folder nesting is too deep",
        ));
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        *entry_count += 1;
        if *entry_count > 100_000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The archive contains too many files",
            ));
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            validate_safe_symlink(&entry.path(), archive_root)?;
        }
        if file_type.is_dir() {
            validate_extracted_tree(
                archive_root,
                &entry.path(),
                depth + 1,
                entry_count,
                extracted_bytes,
            )?;
        } else if file_type.is_file() {
            *extracted_bytes += entry.metadata()?.len();
            if *extracted_bytes > 1024 * 1024 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The extracted archive exceeds 1 GiB",
                ));
            }
        }
    }
    Ok(())
}

fn validate_safe_symlink(path: &Path, allowed_root: &Path) -> io::Result<PathBuf> {
    let target = fs::read_link(path)?;
    if target.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The archive contains an absolute symbolic link",
        ));
    }
    match fs::canonicalize(path) {
        Ok(resolved_target) if resolved_target.starts_with(allowed_root) => {}
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The archive contains a symbolic link that points outside its theme folder",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            validate_broken_internal_symlink(path, &target, allowed_root)?;
        }
        Err(error) => return Err(error),
    }
    Ok(target)
}

fn validate_broken_internal_symlink(
    path: &Path,
    target: &Path,
    allowed_root: &Path,
) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "The symbolic link has no parent",
        )
    })?;
    let relative_parent = parent.strip_prefix(allowed_root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "The symbolic link is outside its allowed theme directory",
        )
    })?;
    let mut relative_target = PathBuf::new();
    for component in relative_parent.components().chain(target.components()) {
        match component {
            std::path::Component::Normal(part) => relative_target.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if relative_target.pop() => {}
            std::path::Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The archive contains a symbolic link that points outside its theme folder",
                ));
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The archive contains an absolute symbolic link",
                ));
            }
        }
    }

    let mut ancestor = allowed_root.join(relative_target);
    loop {
        match fs::symlink_metadata(&ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let resolved = fs::canonicalize(&ancestor).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("The broken link traverses an unresolved symbolic link: {error}"),
                    )
                })?;
                if !resolved.starts_with(allowed_root) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "The archive contains a symbolic link that points outside its theme folder",
                    ));
                }
                return Ok(());
            }
            Ok(_) => {
                let resolved = fs::canonicalize(&ancestor)?;
                if !resolved.starts_with(allowed_root) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "The archive contains a symbolic link that points outside its theme folder",
                    ));
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && ancestor != allowed_root => {
                if !ancestor.pop() {
                    break;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn find_theme_directories(
    directory: &Path,
    theme_type: &str,
    depth: usize,
    candidates: &mut Vec<PathBuf>,
) -> io::Result<()> {
    if depth > 12 {
        return Ok(());
    }
    if is_valid_theme_directory(directory, theme_type)? {
        candidates.push(directory.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            find_theme_directories(&entry.path(), theme_type, depth + 1, candidates)?;
        }
    }
    Ok(())
}

fn is_valid_theme_directory(directory: &Path, theme_type: &str) -> io::Result<bool> {
    let index_path = directory.join("index.theme");
    let index_metadata = match fs::symlink_metadata(&index_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if index_metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The theme index is a symbolic link",
        ));
    }
    if !index_metadata.is_file() {
        return Ok(false);
    }
    let contents = fs::read_to_string(index_path)?;
    if read_ini_value(&contents, "Icon Theme", "Name").is_none_or(|name| name.trim().is_empty()) {
        return Ok(false);
    }
    if theme_type == "cursors" {
        let cursor_directory = directory.join("cursors");
        if !cursor_directory.is_dir()
            || fs::symlink_metadata(&cursor_directory)?
                .file_type()
                .is_symlink()
        {
            return Ok(false);
        }
        let canonical_theme = fs::canonicalize(directory)?;
        for entry in cursor_directory.read_dir()? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_file() {
                return Ok(true);
            }
            if kind.is_symlink() {
                validate_safe_symlink(&entry.path(), &canonical_theme)?;
                match fs::canonicalize(entry.path()) {
                    Ok(target) if target.starts_with(&canonical_theme) && target.is_file() => {
                        return Ok(true);
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
        return Ok(false);
    }

    let directories = read_ini_value(&contents, "Icon Theme", "Directories")
        .unwrap_or_default()
        .split(',')
        .filter_map(safe_relative_path)
        .any(|relative| {
            let path = directory.join(relative);
            path.is_dir()
                && fs::symlink_metadata(path)
                    .is_ok_and(|metadata| !metadata.file_type().is_symlink())
        });
    Ok(directories)
}

fn safe_relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value.trim());
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(path.to_path_buf())
}

fn safe_theme_id(value: &str) -> Option<String> {
    let safe = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let safe = safe.trim_matches(['-', '.']);
    (!safe.is_empty() && safe.len() <= 128).then(|| safe.to_string())
}

fn archive_theme_id(archive: &Path) -> Option<String> {
    let filename = archive.file_name()?.to_string_lossy();
    let mut stem = filename.to_string();
    for suffix in [
        ".tar.gz",
        ".tar.xz",
        ".tar.bz",
        ".tar.bz2",
        ".tar.lz",
        ".tar.lzma",
        ".tar.zst",
        ".tgz",
        ".txz",
        ".tbz2",
        ".tlz",
        ".tzst",
    ] {
        if stem.to_ascii_lowercase().ends_with(suffix) {
            stem.truncate(stem.len() - suffix.len());
            break;
        }
    }
    if stem == filename {
        stem = archive.file_stem()?.to_string_lossy().to_string();
    }
    safe_theme_id(&stem)
}

fn copy_theme_tree(source: &Path, destination: &Path, theme_root: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_theme_tree(&entry.path(), &target, theme_root)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target)?;
        } else if file_type.is_symlink() {
            let link_target = validate_safe_symlink(&entry.path(), theme_root)?;
            match fs::canonicalize(entry.path()) {
                Ok(_) => symlink(link_target, target)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The theme contains unsupported filesystem entries",
            ));
        }
    }
    Ok(())
}

fn make_hidden_theme_selectable(theme_root: &Path, folder_name: &str) -> io::Result<bool> {
    let index_path = theme_root.join("index.theme");
    let contents = fs::read_to_string(&index_path)?;
    if !read_ini_value(&contents, "Icon Theme", "Hidden")
        .is_some_and(|hidden| hidden.eq_ignore_ascii_case("true"))
    {
        return Ok(false);
    }
    let mut group = String::new();
    let mut updated = String::with_capacity(contents.len());
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            group = trimmed[1..trimmed.len() - 1].to_string();
            updated.push_str(line);
            updated.push('\n');
            continue;
        }
        if group == "Icon Theme" {
            if trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "Name")
            {
                updated.push_str(&format!("Name={folder_name}\n"));
                continue;
            }
            if trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "Hidden")
            {
                updated.push_str("Hidden=false\n");
                continue;
            }
        }
        updated.push_str(line);
        updated.push('\n');
    }
    fs::write(index_path, updated)?;
    Ok(true)
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
    let output = Command::new("kwriteconfig6")
        .args(["--notify", "--file", file, "--group", group, "--key", key])
        .arg(value)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "KDE could not notify applications about {file} setting {group}/{key}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn delete_kde_setting(file: &str, group: &str, key: &str) -> io::Result<()> {
    let output = Command::new("kwriteconfig6")
        .args([
            "--notify", "--file", file, "--group", group, "--key", key, "--delete",
        ])
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "KDE could not clear {file} setting {group}/{key}: {}",
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
        let actual = read_config_value(
            &user_config_dir()?.join("kdeglobals"),
            "Colors:Header][Inactive",
            key,
        )?
        .unwrap_or_default();
        if actual != value {
            return Err(io::Error::other(format!(
                "KDE reported success, but inactive header color {key} is {actual}"
            )));
        }
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
    Ok(read_config_value(
        &user_config_dir()?.join("plasmarc"),
        "Theme",
        "name",
    )?)
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

fn create_palettes(path: &Path) -> Result<Vec<Palette>, Box<dyn std::error::Error>> {
    let image = ImageReader::open(path)?.with_guessed_format()?.decode()?;
    let sample = image.thumbnail(320, 320).to_rgba8();
    let colors = estimate_wallpaper_colors(&sample);
    Ok(colors
        .into_iter()
        .enumerate()
        .map(|(index, (hue, saturation, lightness, region_hue))| {
            palette_from_color(index, hue, saturation, lightness, region_hue)
        })
        .collect())
}

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
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "wallpaper-palette-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temporary directory");
        path
    }

    #[test]
    fn invalid_xdg_user_directory_uses_home_fallback() {
        let home = PathBuf::from("/home/test-user");

        assert_eq!(
            resolve_xdg_user_dir(Some(PathBuf::new()), Some(home.clone()), ".local/share").unwrap(),
            home.join(".local/share")
        );
        assert_eq!(
            resolve_xdg_user_dir(
                Some(PathBuf::from("relative/path")),
                Some(home.clone()),
                ".config"
            )
            .unwrap(),
            home.join(".config")
        );
        assert_eq!(
            resolve_xdg_user_dir(
                Some(PathBuf::from("/custom/data")),
                Some(home),
                ".local/share"
            )
            .unwrap(),
            PathBuf::from("/custom/data")
        );
    }

    fn test_palette() -> Palette {
        Palette {
            index: 0,
            name: "Wallpaper Palette 01".to_string(),
            panel: "#0B1520".to_string(),
            background: "#101820".to_string(),
            surface: "#18232C".to_string(),
            accent: "#56A6D8".to_string(),
            selection: "#236A98".to_string(),
            foreground: "#F4F4F5".to_string(),
            selection_foreground: "#FFFFFF".to_string(),
        }
    }

    #[test]
    fn extracts_three_distinct_dominant_hues_from_wallpaper() {
        let sample = RgbaImage::from_fn(90, 30, |x, _| match x / 30 {
            0 => image::Rgba([240, 40, 30, 255]),
            1 => image::Rgba([245, 210, 30, 255]),
            _ => image::Rgba([220, 40, 130, 255]),
        });

        let colors = estimate_wallpaper_colors(&sample);
        assert_eq!(colors.len(), 3);
        for expected_hue in [0.0, 55.0, 330.0] {
            assert!(
                colors
                    .iter()
                    .any(|(hue, _, _, _)| circular_hue_distance(*hue, expected_hue) < 20.0),
                "missing hue near {expected_hue}: {colors:?}"
            );
        }
    }

    #[test]
    fn ignores_black_majority_when_estimating_wallpaper_colors() {
        let sample = RgbaImage::from_fn(100, 10, |x, _| {
            if x < 70 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([16, 46, 84, 255])
            }
        });

        let colors = estimate_wallpaper_colors(&sample);
        assert_eq!(colors.len(), 3);
        assert!(
            colors.iter().any(|(hue, saturation, _, _)| {
                (200.0..230.0).contains(hue) && *saturation > 0.4
            }),
            "blue hue missing: {colors:?}"
        );
    }

    #[test]
    fn neutral_wallpaper_uses_stable_blue_fallback() {
        let sample = RgbaImage::from_pixel(8, 8, image::Rgba([12, 12, 12, 255]));
        let colors = estimate_wallpaper_colors(&sample);
        assert_eq!(colors.len(), 3);
        assert!(circular_hue_distance(colors[0].0, 220.0) < 5.0);
        assert!((colors[0].1 - 0.35).abs() < f64::EPSILON);
    }

    #[test]
    fn preserves_dark_navy_wallpaper_shade_in_generated_background() {
        let sample = RgbaImage::from_pixel(32, 32, image::Rgba([10, 28, 48, 255]));
        let colors = estimate_wallpaper_colors(&sample);
        let (hue, saturation, lightness, region_hue) = colors[0];
        let palette = palette_from_color(0, hue, saturation, lightness, region_hue);
        let value = u32::from_str_radix(&palette.background[1..], 16).unwrap();
        let rgb = (
            f64::from((value >> 16) & 0xff) / 255.0,
            f64::from((value >> 8) & 0xff) / 255.0,
            f64::from(value & 0xff) / 255.0,
        );
        let (background_hue, background_saturation, background_lightness) =
            rgb_to_hsl(rgb.0, rgb.1, rgb.2);

        assert!(circular_hue_distance(background_hue, 210.0) < 10.0);
        assert!(background_saturation > 0.16);
        assert!((0.12..0.20).contains(&background_lightness));
    }

    #[test]
    fn samples_a_matching_hue_from_wallpaper_edges() {
        let sample = RgbaImage::from_fn(20, 20, |_, y| {
            if y < 10 {
                image::Rgba([220, 40, 130, 255])
            } else {
                image::Rgba([16, 46, 84, 255])
            }
        });
        let edge_hue = strongest_edge_hue(&sample, 220.0).unwrap();
        assert!(circular_hue_distance(edge_hue, 220.0) < 10.0);
    }

    #[test]
    fn recolors_panel_svg_gradient_stops() {
        let svg = r#"<linearGradient><stop style="stop-color:#AA0000;stop-opacity:1"/><stop style="stop-color:#550000"/></linearGradient>"#;
        let recolored = recolor_panel_svg(svg, "#12AB34", "#050607").unwrap();
        assert!(recolored.contains("stop-color:#12AB34"));
        assert!(recolored.contains("stop-color:#050607"));
        assert!(!recolored.contains("#AA0000"));
    }

    #[test]
    fn recognizes_only_widget_owned_legacy_scheme_files() {
        let directory = temp_dir("scheme-owner");
        let owned = directory.join("WallpaperPalette02-3.colors");
        fs::write(
            &owned,
            "[General]\nName=Wallpaper Palette 02 (3)\n[Colors:View]\n",
        )
        .unwrap();
        let unrelated = directory.join("WallpaperPalette02-4.colors");
        fs::write(&unrelated, "[General]\nName=My custom palette\n").unwrap();
        let marked = directory.join("AnyName.colors");
        fs::write(
            &marked,
            format!("[General]\nX-WallpaperPalette-Owner={LEGACY_WIDGET_OWNER_MARKER}\n"),
        )
        .unwrap();
        let current = directory.join("PlasmaColorPicker03-2.colors");
        fs::write(&current, "[General]\nName=PlasmaColorPicker 03 (2)\n").unwrap();
        let current_marker = directory.join("MarkedByPcp.colors");
        fs::write(
            &current_marker,
            format!("[General]\nX-WallpaperPalette-Owner={WIDGET_OWNER_MARKER}\n"),
        )
        .unwrap();

        assert!(is_owned_scheme_file(&owned));
        assert!(!is_owned_scheme_file(&unrelated));
        assert!(is_owned_scheme_file(&marked));
        assert!(is_owned_scheme_file(&current));
        assert!(is_owned_scheme_file(&current_marker));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn writes_full_kde_scheme_roles_and_widget_marker() {
        let contents = colors_file("Test Palette", &test_palette());
        for group in [
            "[Colors:Window]",
            "[Colors:Button]",
            "[Colors:View]",
            "[Colors:Tooltip]",
            "[Colors:Complementary]",
            "[Colors:Header]",
            "[Colors:Header][Inactive]",
            "[Colors:Selection]",
            "[ColorEffects:Disabled]",
            "[ColorEffects:Inactive]",
            "[WM]",
        ] {
            assert!(contents.contains(group), "Missing {group}");
        }
        assert!(contents.contains(&format!(
            "[Colors:Header][Inactive]\nBackgroundNormal={}\nBackgroundAlternate={}\n",
            kde_rgb(&test_palette().panel),
            kde_rgb(&test_palette().background)
        )));
        assert!(contents.contains(&format!(
            "BackgroundNormal={}",
            kde_rgb(&test_palette().panel)
        )));
        assert!(contents.contains(&format!("X-WallpaperPalette-Owner={WIDGET_OWNER_MARKER}")));
    }

    #[test]
    fn emits_plasma_theme_color_roles() {
        let contents = plasma_theme_colors(&test_palette());
        for group in [
            "[Background:Normal]",
            "[Background:Window]",
            "[Foreground:Normal]",
            "[Colors:Button]",
            "[Colors:Selection]",
            "[Colors:View]",
            "[Colors:Window]",
            "[Colors:Header]",
            "[Colors:Complementary]",
            "[Colors:Tooltip]",
        ] {
            assert!(contents.contains(group), "Missing {group}");
        }
        assert!(contents.contains("FocusColor=86,166,216"));
    }

    #[test]
    fn decodes_local_file_urls() {
        assert_eq!(
            local_file_path("file:///home/test/My%20Wallpaper.png"),
            Some(PathBuf::from("/home/test/My Wallpaper.png"))
        );
        assert_eq!(local_file_path("https://example.test/image.png"), None);
    }

    #[test]
    fn validates_icon_and_cursor_theme_archives() {
        let directory = temp_dir("theme-validation");
        let icon_theme = directory.join("icons");
        fs::create_dir_all(icon_theme.join("16x16/apps")).unwrap();
        fs::write(
            icon_theme.join("index.theme"),
            "[Icon Theme]\nName=Test Icons\nDirectories=16x16/apps\n",
        )
        .unwrap();
        assert!(is_valid_theme_directory(&icon_theme, "icons").unwrap());
        assert!(!is_valid_theme_directory(&icon_theme, "cursors").unwrap());

        let cursor_theme = directory.join("cursors-theme");
        fs::create_dir_all(cursor_theme.join("cursors")).unwrap();
        fs::write(
            cursor_theme.join("index.theme"),
            "[Icon Theme]\nName=Test Cursor\n",
        )
        .unwrap();
        fs::write(cursor_theme.join("cursors/default"), b"cursor").unwrap();
        assert!(is_valid_theme_directory(&cursor_theme, "cursors").unwrap());
        assert!(!is_valid_theme_directory(&cursor_theme, "icons").unwrap());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recognizes_cursor_themes_whose_cursor_names_are_internal_symlinks() {
        let directory = temp_dir("cursor-symlink-validation");
        let cursor_theme = directory.join("cursor-theme");
        fs::create_dir_all(cursor_theme.join("cursors")).unwrap();
        fs::create_dir_all(cursor_theme.join("cursor-data")).unwrap();
        fs::write(cursor_theme.join("cursor-data/default"), b"cursor").unwrap();
        fs::write(
            cursor_theme.join("index.theme"),
            "[Icon Theme]\nName=Linked Cursor Theme\n",
        )
        .unwrap();
        symlink(
            "../cursor-data/default",
            cursor_theme.join("cursors/default"),
        )
        .unwrap();

        assert!(is_valid_theme_directory(&cursor_theme, "cursors").unwrap());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn installs_cursors_in_the_xcursor_user_directory() {
        let data_home = Path::new("/home/test/.local/share");
        let home = Path::new("/home/test");
        assert_eq!(
            theme_install_directory_for("icons", data_home, home).unwrap(),
            PathBuf::from("/home/test/.local/share/icons")
        );
        assert_eq!(
            theme_install_directory_for("cursors", data_home, home).unwrap(),
            PathBuf::from("/home/test/.icons")
        );
        assert!(theme_install_directory_for("themes", data_home, home).is_err());
    }

    #[test]
    fn detects_tar_layers_by_header_even_without_a_tar_extension() {
        let directory = temp_dir("tar-magic-detection");
        let payload = directory.join("extensionless-payload");
        let mut header = [0u8; 512];
        header[..4].copy_from_slice(b"test");
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(b"00000000000\0");
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
        let checksum_field = format!("{checksum:06o}\0 ");
        header[148..156].copy_from_slice(checksum_field.as_bytes());
        fs::write(&payload, header).unwrap();
        assert!(is_tar_archive(&payload).unwrap());
        assert_eq!(find_inner_tar(&directory).unwrap(), Some(payload.clone()));

        fs::write(&payload, b"not a tar archive").unwrap();
        assert!(!is_tar_archive(&payload).unwrap());
        assert_eq!(find_inner_tar(&directory).unwrap(), None);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn allows_internal_theme_symlinks_and_rejects_external_targets() {
        let directory = temp_dir("theme-symlinks");
        let theme = directory.join("theme");
        fs::create_dir_all(theme.join("cursors")).unwrap();
        fs::write(theme.join("cursors/default"), b"cursor").unwrap();
        symlink("default", theme.join("cursors/left_ptr")).unwrap();

        let canonical_root = fs::canonicalize(&directory).unwrap();
        let link_target =
            validate_safe_symlink(&theme.join("cursors/left_ptr"), &canonical_root).unwrap();
        assert_eq!(link_target, PathBuf::from("default"));

        let copied = directory.join("copied");
        let canonical_theme = fs::canonicalize(&theme).unwrap();
        copy_theme_tree(&theme, &copied, &canonical_theme).unwrap();
        assert_eq!(
            fs::read_link(copied.join("cursors/left_ptr")).unwrap(),
            PathBuf::from("default")
        );

        symlink("missing-cursor", theme.join("cursors/unused-alias")).unwrap();
        assert!(
            validate_safe_symlink(&theme.join("cursors/unused-alias"), &canonical_theme).is_ok()
        );
        copy_theme_tree(
            &theme,
            &directory.join("copied-with-broken-alias"),
            &canonical_theme,
        )
        .unwrap();
        assert!(!directory
            .join("copied-with-broken-alias/cursors/unused-alias")
            .exists());

        let outside = directory.join("outside");
        fs::write(&outside, b"outside").unwrap();
        symlink("../../outside", theme.join("cursors/escape")).unwrap();
        assert!(validate_safe_symlink(&theme.join("cursors/escape"), &canonical_theme).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn assigns_a_non_conflicting_theme_folder_name() {
        let directory = temp_dir("theme-name-conflict");
        fs::create_dir(directory.join("Kora-Yellow")).unwrap();
        assert_eq!(
            unique_theme_folder_name("Kora-Yellow", &[directory.clone()], &[]).unwrap(),
            "Kora-Yellow-PCP"
        );
        assert_eq!(
            unique_theme_folder_name(
                "Kora-Yellow",
                &[directory.clone()],
                &["Kora-Yellow-PCP".to_string()]
            )
            .unwrap(),
            "Kora-Yellow-PCP-2"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rewrites_theme_inheritance_when_a_folder_name_changes() {
        let directory = temp_dir("theme-id-rewrite");
        fs::write(
            directory.join("index.theme"),
            "[Icon Theme]\nName=Kora\nInherits=hicolor,Another\n",
        )
        .unwrap();
        rewrite_theme_index_for_install(
            &directory,
            "Kora",
            "Kora-PCP",
            &[("Another".to_string(), "Another-PCP".to_string())],
        )
        .unwrap();
        let contents = fs::read_to_string(directory.join("index.theme")).unwrap();
        assert!(contents.contains("Name=Kora (PCP)"));
        assert!(contents.contains("Inherits=hicolor,Another-PCP"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exposes_single_hidden_theme_when_staging_it() {
        let directory = temp_dir("hidden-theme");
        fs::write(
            directory.join("index.theme"),
            "[Icon Theme]\nName=Adwaita\nHidden=true\nDirectories=16x16/apps\n\n[16x16/apps]\nSize=16\n",
        )
        .unwrap();
        assert!(make_hidden_theme_selectable(&directory, "Mojito").unwrap());
        let contents = fs::read_to_string(directory.join("index.theme")).unwrap();
        assert!(contents.contains("Name=Mojito"));
        assert!(contents.contains("Hidden=false"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_unsafe_theme_directory_entries() {
        assert!(safe_relative_path("16x16/apps").is_some());
        assert!(safe_relative_path("../outside").is_none());
        assert!(safe_relative_path("/absolute/path").is_none());
        assert_eq!(
            safe_theme_id("../Test theme"),
            Some("Test-theme".to_string())
        );
        assert_eq!(
            archive_theme_id(Path::new("/tmp/Mojito.tar.zst")),
            Some("Mojito".to_string())
        );
    }

    #[test]
    fn selects_readable_text_for_selection_backgrounds() {
        assert_eq!(contrast_foreground((0.05, 0.08, 0.1)), "#FFFFFF");
        assert_eq!(contrast_foreground((0.9, 0.9, 0.9)), "#000000");
    }

    #[test]
    fn maps_palette_accent_to_kde_store_search_color() {
        assert_eq!(hue_search_term("#D93B38").unwrap(), "red");
        assert_eq!(hue_search_term("#4C88BB").unwrap(), "blue");
        assert_eq!(hue_search_term("#D62A94").unwrap(), "pink");
        assert_eq!(hue_search_term("#DEB352").unwrap(), "yellow");
        assert_eq!(hue_search_term("#DF8657").unwrap(), "orange");
        assert_eq!(hue_search_term("#E8C547").unwrap(), "yellow");
        assert!(hue_search_term("blue").is_err());
    }

    #[test]
    fn assigns_distinct_names_to_repeated_saves() {
        assert_eq!(
            scheme_display_name("WallpaperPalette02", 1),
            "Wallpaper Palette 02"
        );
        assert_eq!(
            scheme_display_name("WallpaperPalette02-3", 1),
            "Wallpaper Palette 02 (3)"
        );
        assert_eq!(
            scheme_display_name("PlasmaColorPicker02", 1),
            "PlasmaColorPicker 02"
        );
        assert_eq!(
            scheme_display_name("PlasmaColorPicker02-3", 1),
            "PlasmaColorPicker 02 (3)"
        );
    }

    #[test]
    fn generates_a_new_theme_id_for_each_saved_palette() {
        let first = generated_theme_id_for_scheme("PlasmaColorPicker02");
        let second = generated_theme_id_for_scheme("PlasmaColorPicker03-2");

        assert!(is_generated_theme_id(&first));
        assert!(is_generated_theme_id(&second));
        assert_ne!(first, second);
        assert_eq!(generated_theme_id_for_scheme("PlasmaColorPicker02"), first);
    }

    #[test]
    fn writes_required_kde_color_groups() {
        let contents = colors_file("PlasmaColorPicker 01", &test_palette());
        assert!(contents.contains("[Colors:Window]"));
        assert!(contents.contains("[Colors:Selection]"));
        assert!(contents.contains("[WM]"));
    }
}

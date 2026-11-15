//! Safely extract, validate, and install icon or cursor theme archives.

use super::*;

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

pub(super) fn install_theme(
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
    let allowed_theme_roots = icon_theme_directories()?
        .into_iter()
        .filter_map(|directory| fs::canonicalize(directory).ok())
        .collect::<Vec<_>>();
    materialize_absolute_theme_symlinks(&staging_root, &allowed_theme_roots)?;
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

pub(super) fn materialize_absolute_theme_symlinks(
    directory: &Path,
    allowed_roots: &[PathBuf],
) -> io::Result<()> {
    let mut entry_count = 0usize;
    let mut copied_bytes = 0u64;
    materialize_absolute_theme_symlinks_in(
        directory,
        allowed_roots,
        0,
        &mut entry_count,
        &mut copied_bytes,
    )
}

pub(super) fn materialize_absolute_theme_symlinks_in(
    directory: &Path,
    allowed_roots: &[PathBuf],
    depth: usize,
    entry_count: &mut usize,
    copied_bytes: &mut u64,
) -> io::Result<()> {
    if depth > 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The archive folder nesting is too deep",
        ));
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        *entry_count += 1;
        if *entry_count > 100_000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The archive contains too many files",
            ));
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let target = fs::read_link(&path)?;
            if target.is_absolute() {
                match fs::canonicalize(&target) {
                    Ok(canonical_target)
                        if allowed_roots
                            .iter()
                            .any(|root| canonical_target.starts_with(root)) =>
                    {
                        let mut ancestors = Vec::new();
                        copy_allowed_theme_target(
                            &canonical_target,
                            &path,
                            allowed_roots,
                            &mut ancestors,
                            entry_count,
                            copied_bytes,
                        )?;
                    }
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "The archive contains an absolute symbolic link outside installed icon and cursor themes",
                        ));
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        remove_file_if_present(&path)?;
                    }
                    Err(error) => return Err(error),
                }
            }
        } else if file_type.is_dir() {
            materialize_absolute_theme_symlinks_in(
                &path,
                allowed_roots,
                depth + 1,
                entry_count,
                copied_bytes,
            )?;
        }
    }
    Ok(())
}

pub(super) fn copy_allowed_theme_target(
    source: &Path,
    destination: &Path,
    allowed_roots: &[PathBuf],
    ancestors: &mut Vec<PathBuf>,
    entry_count: &mut usize,
    copied_bytes: &mut u64,
) -> io::Result<()> {
    if ancestors.len() > 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The linked theme folder nesting is too deep",
        ));
    }
    let source = fs::canonicalize(source)?;
    if !allowed_roots.iter().any(|root| source.starts_with(root)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The archive contains a symbolic link outside installed icon and cursor themes",
        ));
    }
    if ancestors.contains(&source) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The archive contains a circular symbolic link in an installed theme",
        ));
    }

    let metadata = fs::metadata(&source)?;
    if metadata.is_file() {
        *entry_count += 1;
        *copied_bytes += metadata.len();
        if *entry_count > 100_000 || *copied_bytes > 1024 * 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The linked theme assets exceed the archive safety limits",
            ));
        }
        remove_file_if_present(destination)?;
        fs::copy(source, destination)?;
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The archive links to an unsupported entry in an installed theme",
        ));
    }

    *entry_count += 1;
    if *entry_count > 100_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The linked theme assets exceed the archive safety limits",
        ));
    }
    remove_file_if_present(destination)?;
    fs::create_dir(destination)?;
    ancestors.push(source.clone());
    for entry in fs::read_dir(&source)? {
        let entry = entry?;
        let source_child = entry.path();
        let destination_child = destination.join(entry.file_name());
        let source_type = entry.file_type()?;
        if source_type.is_symlink() {
            match fs::canonicalize(&source_child) {
                Ok(target) => copy_allowed_theme_target(
                    &target,
                    &destination_child,
                    allowed_roots,
                    ancestors,
                    entry_count,
                    copied_bytes,
                )?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        } else {
            copy_allowed_theme_target(
                &source_child,
                &destination_child,
                allowed_roots,
                ancestors,
                entry_count,
                copied_bytes,
            )?;
        }
    }
    ancestors.pop();
    Ok(())
}

pub(super) fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn extract_theme_archive(
    unar: Option<&Path>,
    seven_zip: Option<&Path>,
    archive: &Path,
    theme_type: &str,
    destination: &Path,
) -> io::Result<()> {
    let extract =
        |input: &Path, output: &Path| extract_with_available_tools(unar, seven_zip, input, output);
    extract(archive, destination)?;
    for _ in 0..8 {
        let mut themes = Vec::new();
        find_theme_directories(destination, theme_type, 0, &mut themes)?;
        if !themes.is_empty() {
            return Ok(());
        }
        let nested_archives = find_nested_archives(destination)?;
        if nested_archives.is_empty() {
            return Ok(());
        }
        for nested_archive in nested_archives {
            let nested_layer = create_nested_archive_directory(&nested_archive)?;
            extract(&nested_archive, &nested_layer)?;
            fs::remove_file(nested_archive)?;
        }
    }
    if !find_nested_archives(destination)?.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The archive contains too many nested archive layers",
        ));
    }
    Ok(())
}

pub(super) fn extract_with_available_tools(
    unar: Option<&Path>,
    seven_zip: Option<&Path>,
    archive: &Path,
    destination: &Path,
) -> io::Result<()> {
    if let Some(unar) = unar {
        match extract_with_unar(unar, archive, destination) {
            Ok(()) => return Ok(()),
            Err(unar_error) => {
                if let Some(seven_zip) = seven_zip {
                    clear_directory_contents(destination)?;
                    return extract_with_7z_checked(seven_zip, archive, destination).map_err(
                        |seven_zip_error| {
                            io::Error::other(format!(
                                "{unar_error}; 7z fallback also failed: {seven_zip_error}"
                            ))
                        },
                    );
                }
                return Err(unar_error);
            }
        }
    }

    let seven_zip = seven_zip.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Neither unar nor 7z is available")
    })?;
    extract_with_7z_checked(seven_zip, archive, destination)
}

pub(super) fn clear_directory_contents(directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if fs::symlink_metadata(&path)?.file_type().is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub(super) fn extract_with_7z_checked(
    seven_zip: &Path,
    archive: &Path,
    destination: &Path,
) -> io::Result<()> {
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

pub(super) fn find_nested_archives(directory: &Path) -> io::Result<Vec<PathBuf>> {
    fn visit(directory: &Path, archives: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                visit(&entry.path(), archives)?;
            } else if file_type.is_file() && is_supported_archive(&entry.path())? {
                archives.push(entry.path());
            }
        }
        Ok(())
    }

    let mut archives = Vec::new();
    visit(directory, &mut archives)?;
    archives.sort();
    Ok(archives)
}

pub(super) fn is_supported_archive(path: &Path) -> io::Result<bool> {
    const ARCHIVE_SUFFIXES: &[&str] = &[
        ".tar.gz",
        ".tar.xz",
        ".tar.bz",
        ".tar.bz2",
        ".tar.z",
        ".tar.lzo",
        ".tar.lz",
        ".tar.lzma",
        ".tar.lz4",
        ".tar.zst",
        ".tar.br",
        ".tgz",
        ".txz",
        ".tbz",
        ".tbz2",
        ".tlz",
        ".tzst",
        ".zip",
        ".zipx",
        ".7z",
        ".rar",
        ".arj",
        ".ace",
        ".ar",
        ".arc",
        ".cab",
        ".chm",
        ".cpio",
        ".deb",
        ".dmg",
        ".esd",
        ".hfs",
        ".iso",
        ".img",
        ".jar",
        ".lha",
        ".lzh",
        ".lzo",
        ".lzx",
        ".msi",
        ".nsis",
        ".pkg",
        ".rpm",
        ".squashfs",
        ".swf",
        ".swm",
        ".vhd",
        ".vhdx",
        ".vdi",
        ".vmdk",
        ".qcow",
        ".qcow2",
        ".simg",
        ".xip",
        ".xpi",
        ".wim",
        ".xar",
        ".zoo",
        ".001",
        ".gz",
        ".xz",
        ".bz",
        ".bz2",
        ".lz",
        ".lzma",
        ".lz4",
        ".lzo",
        ".zst",
        ".br",
        ".z",
        ".zip.001",
        ".rar.001",
        ".tar",
    ];
    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if ARCHIVE_SUFFIXES
        .iter()
        .any(|suffix| filename.ends_with(suffix))
    {
        return Ok(true);
    }

    let mut file = fs::File::open(path)?;
    let mut signature = [0u8; 8];
    let read = io::Read::read(&mut file, &mut signature)?;
    let signature = &signature[..read];
    let standard_signature = signature.starts_with(b"PK\x03\x04")
        || signature.starts_with(b"PK\x05\x06")
        || signature.starts_with(b"PK\x06\x06")
        || signature.starts_with(b"PK\x06\x08")
        || signature.starts_with(b"PK\x07\x08")
        || signature.starts_with(b"7z\xbc\xaf\x27\x1c")
        || signature.starts_with(b"Rar!\x1a\x07")
        || signature.starts_with(b"\x1f\x8b")
        || signature.starts_with(b"\x1f\x9d")
        || signature.starts_with(b"\xfd7zXZ\x00")
        || signature.starts_with(b"BZh")
        || signature.starts_with(b"\x28\xb5\x2f\xfd")
        || signature.starts_with(b"\x04\x22\x4d\x18")
        || signature.starts_with(b"LZIP")
        || signature.starts_with(b"MSCF")
        || signature.starts_with(b"MSWIM")
        || signature.starts_with(b"ITSF")
        || signature.starts_with(b"xar!")
        || signature.starts_with(b"**ACE**")
        || signature.starts_with(b"!<arch>\n")
        || signature.starts_with(b"\x60\xea")
        || signature.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1")
        || signature.starts_with(b"\xed\xab\xee\xdb")
        || signature.starts_with(b"koly\x00\x00\x00\x04")
        || signature.starts_with(b"070701")
        || signature.starts_with(b"070702")
        || signature.starts_with(b"070707");
    if standard_signature || is_tar_archive(path)? {
        return Ok(true);
    }

    let mut file = fs::File::open(path)?;
    if file.seek(io::SeekFrom::Start(32_769)).is_err() {
        return Ok(false);
    }
    let mut iso_signature = [0u8; 5];
    Ok(io::Read::read_exact(&mut file, &mut iso_signature).is_ok() && &iso_signature == b"CD001")
}

pub(super) fn create_nested_archive_directory(archive: &Path) -> io::Result<PathBuf> {
    let parent = archive.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "The nested archive has no parent directory",
        )
    })?;
    let base = archive_theme_id(archive).unwrap_or_else(|| "nested-theme".to_string());
    for suffix in 0..=999 {
        let name = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-nested-{suffix}")
        };
        let destination = parent.join(name);
        match fs::create_dir(&destination) {
            Ok(()) => return Ok(destination),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Could not create a directory for the nested archive",
    ))
}

pub(super) fn is_tar_archive(path: &Path) -> io::Result<bool> {
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

pub(super) fn extract_with_unar(unar: &Path, archive: &Path, destination: &Path) -> io::Result<()> {
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

pub(super) fn extract_with_7z(
    seven_zip: &Path,
    archive: &Path,
    destination: &Path,
) -> io::Result<()> {
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

pub(super) fn format_extraction_error(extractor: &str, stderr: &[u8]) -> String {
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

pub(super) fn icon_theme_directories() -> io::Result<Vec<PathBuf>> {
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

pub(super) fn theme_install_directory(theme_type: &str) -> io::Result<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Cannot determine home directory")
    })?;
    let data_home = user_data_dir()?;
    theme_install_directory_for(theme_type, &data_home, &home)
}

pub(super) fn theme_install_directory_for(
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

pub(super) fn unique_theme_folder_name(
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

pub(super) fn rewrite_theme_index_for_install(
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

pub(super) fn renamed_theme_id<'a>(
    theme_id: &'a str,
    renamed: &'a [(String, String)],
) -> Option<&'a str> {
    renamed
        .iter()
        .find(|(original, _)| original == theme_id)
        .map(|(_, replacement)| replacement.as_str())
}

pub(super) fn validate_extracted_tree(
    archive_root: &Path,
    directory: &Path,
    depth: usize,
    entry_count: &mut usize,
    extracted_bytes: &mut u64,
) -> io::Result<()> {
    if depth > 32 {
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

pub(super) fn validate_safe_symlink(path: &Path, allowed_root: &Path) -> io::Result<PathBuf> {
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

pub(super) fn validate_broken_internal_symlink(
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

pub(super) fn find_theme_directories(
    directory: &Path,
    theme_type: &str,
    depth: usize,
    candidates: &mut Vec<PathBuf>,
) -> io::Result<()> {
    if depth > 32 {
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

pub(super) fn is_valid_theme_directory(directory: &Path, theme_type: &str) -> io::Result<bool> {
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
        for entry in cursor_directory.read_dir()? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_file() || kind.is_symlink() {
                return Ok(true);
            }
        }
        return Ok(false);
    }

    let directories = ["Directories", "ScaledDirectories"]
        .iter()
        .filter_map(|key| read_ini_value(&contents, "Icon Theme", key))
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter_map(|directory| safe_relative_path(&directory))
        .any(|relative| {
            let path = directory.join(relative);
            path.is_dir()
                && fs::symlink_metadata(path)
                    .is_ok_and(|metadata| !metadata.file_type().is_symlink())
        });
    Ok(directories)
}

pub(super) fn safe_relative_path(value: &str) -> Option<PathBuf> {
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

pub(super) fn safe_theme_id(value: &str) -> Option<String> {
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

pub(super) fn archive_theme_id(archive: &Path) -> Option<String> {
    let filename = archive.file_name()?.to_string_lossy();
    let mut stem = filename.to_string();
    for suffix in [
        ".tar.gz",
        ".tar.xz",
        ".tar.bz",
        ".tar.bz2",
        ".tar.z",
        ".tar.lzo",
        ".tar.lz",
        ".tar.lzma",
        ".tar.lz4",
        ".tar.zst",
        ".tar.br",
        ".tgz",
        ".txz",
        ".tbz",
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

pub(super) fn copy_theme_tree(
    source: &Path,
    destination: &Path,
    theme_root: &Path,
) -> io::Result<()> {
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

pub(super) fn make_hidden_theme_selectable(
    theme_root: &Path,
    folder_name: &str,
) -> io::Result<bool> {
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

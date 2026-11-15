//! Regression tests for wallpaper analysis, theme handling, updates, and settings.

use super::*;
use std::collections::BTreeMap;
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
        colors
            .iter()
            .any(|(hue, saturation, _, _)| { (200.0..230.0).contains(hue) && *saturation > 0.4 }),
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
fn video_analysis_ignores_sparse_chroma_compression_artifacts() {
    let frame = RgbaImage::from_fn(100, 100, |x, y| {
        if x == 0 && y == 0 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([140, 140, 140, 255])
        }
    });

    let (palettes, count) = palettes_from_video_frames(vec![frame]).unwrap();

    assert_eq!(count, 1);
    assert_eq!(palettes.len(), 3);
    for palette in palettes {
        assert_eq!(hue_search_term(&palette.accent).unwrap(), "dark");
        let color = u32::from_str_radix(&palette.accent[1..], 16).unwrap();
        let (_, saturation, _) = rgb_to_hsl(
            f64::from((color >> 16) & 0xff) / 255.0,
            f64::from((color >> 8) & 0xff) / 255.0,
            f64::from(color & 0xff) / 255.0,
        );
        assert!(saturation < 0.12);
    }
}

#[test]
fn video_analysis_keeps_dominant_chromatic_content() {
    let frame = RgbaImage::from_fn(100, 100, |x, _| {
        if x < 20 {
            image::Rgba([30, 80, 220, 255])
        } else {
            image::Rgba([140, 140, 140, 255])
        }
    });

    let (palettes, _) = palettes_from_video_frames(vec![frame]).unwrap();

    assert!(
        palettes
            .iter()
            .any(|palette| hue_search_term(&palette.accent).unwrap() == "blue"),
        "dominant blue content was lost: {:?}",
        palettes
            .iter()
            .map(|palette| palette.accent.as_str())
            .collect::<Vec<_>>()
    );
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
fn reads_source_from_the_selected_wallpaper_plugin_only() {
    let config = "[Containments][0]\nformfactor=2\nlastScreen=0\nwallpaperplugin=org.kde.image\n\
        [Containments][0][Wallpaper][org.kde.image][General]\nImage=file:///panel-background.png\n\
        [Containments][1]\nformfactor=0\nlastScreen=0\nwallpaperplugin=org.lumi.livewallpaper\n\
        [Containments][1][Wallpaper][org.kde.image][General]\nImage=file:///old-image.jpg\n\
        [Containments][1][Wallpaper][org.lumi.livewallpaper][General]\nMediaSource=file:///wallpapers/loop%20video.mp4\n";
    let (plugin, settings) = parse_wallpaper_settings(config).unwrap();
    assert_eq!(plugin, "org.lumi.livewallpaper");
    assert_eq!(
        local_path(settings.get("MediaSource").unwrap()),
        Some(PathBuf::from("/wallpapers/loop video.mp4"))
    );
}

#[test]
fn reads_enabled_video_from_smart_video_wallpaper_playlist() {
    let settings = std::collections::BTreeMap::from([(
        "VideoUrls".to_string(),
        r#"[{"filename":"file:///home/test/Загрузки/wallpaper%20one.mp4","enabled":true},{"filename":"file:///home/test/disabled.mp4","enabled":false}]"#.to_string(),
    )]);

    assert_eq!(
        configured_video_paths(&settings).unwrap(),
        vec![PathBuf::from("/home/test/Загрузки/wallpaper one.mp4")]
    );
}

#[test]
fn empty_video_playlist_has_no_source() {
    let settings = std::collections::BTreeMap::from([("VideoUrls".to_string(), "[]".to_string())]);

    assert!(configured_video_paths(&settings).unwrap().is_empty());
}

#[test]
fn samples_multiple_frames_from_animated_gif() {
    let directory = temp_dir("animated-gif");
    let path = directory.join("wallpaper.gif");
    let frames = [
        image::Rgba([240, 30, 30, 255]),
        image::Rgba([30, 230, 30, 255]),
        image::Rgba([30, 30, 240, 255]),
    ]
    .into_iter()
    .map(|color| image::Frame::new(RgbaImage::from_pixel(16, 16, color)))
    .collect::<Vec<_>>();
    image::codecs::gif::GifEncoder::new(fs::File::create(&path).unwrap())
        .encode_frames(frames)
        .unwrap();

    let samples = sample_animated_frames(&path, "gif").unwrap();
    assert_eq!(samples.len(), 3);
    let colors = estimate_wallpaper_colors(&combine_sample_frames(&samples));
    for expected_hue in [0.0, 120.0, 240.0] {
        assert!(
            colors
                .iter()
                .any(|(hue, _, _, _)| { circular_hue_distance(*hue, expected_hue) < 20.0 }),
            "missing hue near {expected_hue}: {colors:?}"
        );
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn samples_video_frames_when_ffmpeg_is_available() {
    let Some(ffmpeg) = find_executable("ffmpeg") else {
        return;
    };
    if find_executable("ffprobe").is_none() {
        return;
    }
    let directory = temp_dir("video-frames");
    let path = directory.join("wallpaper.mp4");
    let output = Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=6",
            "-t",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not generate test video: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let frames = sample_video_frames(&path).unwrap();
    assert!(!frames.is_empty());
    assert!(frames.len() <= 6);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn selects_up_to_eight_images_from_slideshow_directories() {
    let directory = temp_dir("slideshow");
    for index in 0..10 {
        RgbaImage::from_pixel(8, 8, image::Rgba([index * 20, 20, 180, 255]))
            .save(directory.join(format!("{index:02}.png")))
            .unwrap();
    }
    let settings = std::collections::BTreeMap::from([(
        "SlidePaths".to_string(),
        directory.to_string_lossy().to_string(),
    )]);
    let paths = slideshow_image_paths(&settings).unwrap();
    assert_eq!(paths.len(), 8);
    assert!(paths.iter().all(|path| path.is_file()));
    fs::remove_dir_all(directory).unwrap();
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
fn recognizes_icon_themes_with_only_scaled_directories() {
    let directory = temp_dir("scaled-icon-theme");
    let theme = directory.join("scaled-icons");
    fs::create_dir_all(theme.join("32x32/apps")).unwrap();
    fs::write(
        theme.join("index.theme"),
        "[Icon Theme]\nName=Scaled Icons\nScaledDirectories=32x32/apps\n\
         [32x32/apps]\nSize=32\nScale=2\nType=Fixed\n",
    )
    .unwrap();

    assert!(is_valid_theme_directory(&theme, "icons").unwrap());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn retries_archive_extraction_with_7z_after_unar_fails() {
    use std::os::unix::fs::PermissionsExt;

    let directory = temp_dir("extractor-fallback");
    let unar = directory.join("unar");
    let seven_zip = directory.join("7z");
    let archive = directory.join("theme.archive");
    let destination = directory.join("output");
    fs::create_dir(&destination).unwrap();
    fs::write(&archive, b"archive").unwrap();
    fs::write(
        &unar,
        "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n\
         if [ \"$1\" = \"-o\" ]; then shift; mkdir -p \"$1\"; \
         printf partial > \"$1/partial\"; fi\nshift\ndone\nexit 1\n",
    )
    .unwrap();
    fs::write(
        &seven_zip,
        "#!/bin/sh\nfor argument do\ncase \"$argument\" in \
         -o*) destination=${argument#-o};; esac\ndone\n\
         printf extracted > \"$destination/result\"\n",
    )
    .unwrap();
    for extractor in [&unar, &seven_zip] {
        let mut permissions = fs::metadata(extractor).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(extractor, permissions).unwrap();
    }

    extract_with_available_tools(Some(&unar), Some(&seven_zip), &archive, &destination).unwrap();
    assert!(!destination.join("partial").exists());
    assert_eq!(fs::read(destination.join("result")).unwrap(), b"extracted");
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
fn detects_nested_archives_by_header_even_without_an_extension() {
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
    assert!(is_supported_archive(&payload).unwrap());
    assert_eq!(
        find_nested_archives(&directory).unwrap(),
        vec![payload.clone()]
    );

    let nested_directory = directory.join("wrapped");
    fs::create_dir(&nested_directory).unwrap();
    let nested_payload = nested_directory.join("second-payload");
    fs::copy(&payload, &nested_payload).unwrap();
    assert_eq!(
        find_nested_archives(&directory).unwrap(),
        vec![payload.clone(), nested_payload.clone()]
    );

    fs::write(&payload, b"not a tar archive").unwrap();
    assert!(!is_tar_archive(&payload).unwrap());
    assert_eq!(
        find_nested_archives(&directory).unwrap(),
        vec![nested_payload]
    );

    let extensionless_zip = directory.join("archive-without-extension");
    fs::write(&extensionless_zip, b"PK\x03\x04").unwrap();
    assert!(is_supported_archive(&extensionless_zip).unwrap());

    for (name, signature) in [
        ("extensionless-rar", b"Rar!\x1a\x07\x01\x00".as_slice()),
        ("extensionless-cab", b"MSCF\x00\x00\x00\x00".as_slice()),
        ("extensionless-ar", b"!<arch>\n".as_slice()),
        ("extensionless-rpm", b"\xed\xab\xee\xdb".as_slice()),
        ("extensionless-dmg", b"koly\x00\x00\x00\x04".as_slice()),
    ] {
        let archive = directory.join(name);
        fs::write(&archive, signature).unwrap();
        assert!(
            is_supported_archive(&archive).unwrap(),
            "failed to detect {name}"
        );
    }

    let extensionless_iso = directory.join("extensionless-iso");
    let mut iso = vec![0; 32_774];
    iso[32_769..].copy_from_slice(b"CD001");
    fs::write(&extensionless_iso, iso).unwrap();
    assert!(is_supported_archive(&extensionless_iso).unwrap());

    let extension_only_archive = directory.join("theme.lzh");
    fs::write(&extension_only_archive, b"").unwrap();
    assert!(is_supported_archive(&extension_only_archive).unwrap());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn loads_legacy_user_state_without_appearance_snapshots() {
    let state: UserState = serde_json::from_str(
        r#"{"previous_color_scheme":"BreezeDark","previous_desktop_theme":"breeze"}"#,
    )
    .unwrap();
    assert_eq!(state.previous_accent_color, None);
    assert_eq!(state.previous_inactive_header_background, None);
    assert_eq!(state.previous_inactive_header_alternate, None);
    assert!(!state.appearance_settings_captured);
}

#[test]
fn reads_desktop_theme_from_look_and_feel_defaults() {
    let defaults = "\
[kdeglobals][Icons]
Theme=breeze-dark

[plasmarc][Theme]
name=default

[kwinrc][org.kde.kdecoration2]
theme=org.kde.breeze
";

    assert_eq!(
        read_kde_defaults_value(defaults, "plasmarc", "Theme", "name"),
        Some("default".to_string())
    );
    assert_eq!(
        read_kde_defaults_value(defaults, "kdeglobals", "Icons", "Theme"),
        Some("breeze-dark".to_string())
    );
    assert_eq!(
        read_kde_defaults_value(defaults, "plasmarc", "Theme", "missing"),
        None
    );
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
    assert!(validate_safe_symlink(&theme.join("cursors/unused-alias"), &canonical_theme).is_ok());
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
fn materializes_absolute_links_to_assets_in_installed_icon_themes() {
    let directory = temp_dir("absolute-theme-links");
    let icon_root = directory.join("share/icons");
    let target_theme = icon_root.join("breeze-dark/places/22");
    let extracted_theme = directory.join("extracted/Slot-Theme");
    fs::create_dir_all(&target_theme).unwrap();
    fs::create_dir_all(extracted_theme.join("22/places")).unwrap();
    fs::write(target_theme.join("folder-library.svg"), b"<svg/>").unwrap();
    fs::write(
        extracted_theme.join("index.theme"),
        "[Icon Theme]\nName=Slot Theme\nDirectories=22/places\n",
    )
    .unwrap();
    symlink(
        target_theme.join("folder-library.svg"),
        extracted_theme.join("22/places/folder-library.svg"),
    )
    .unwrap();

    let allowed_roots = vec![fs::canonicalize(&icon_root).unwrap()];
    materialize_absolute_theme_symlinks(
        &fs::canonicalize(directory.join("extracted")).unwrap(),
        &allowed_roots,
    )
    .unwrap();

    let icon = extracted_theme.join("22/places/folder-library.svg");
    assert!(!fs::symlink_metadata(&icon)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(icon).unwrap(), b"<svg/>");
    let extracted_root = fs::canonicalize(directory.join("extracted")).unwrap();
    let mut entry_count = 0;
    let mut extracted_bytes = 0;
    validate_extracted_tree(
        &extracted_root,
        &extracted_root,
        0,
        &mut entry_count,
        &mut extracted_bytes,
    )
    .unwrap();
    assert!(is_valid_theme_directory(&extracted_theme, "icons").unwrap());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn materializes_absolute_directory_links_within_installed_icon_themes() {
    let directory = temp_dir("absolute-theme-directory-link");
    let icon_root = directory.join("share/icons");
    let target_theme = icon_root.join("breeze-dark/24/apps");
    let extracted_theme = directory.join("extracted/Slot-Theme");
    fs::create_dir_all(&target_theme).unwrap();
    fs::create_dir_all(extracted_theme.join("24")).unwrap();
    fs::write(target_theme.join("app.svg"), b"<svg/>").unwrap();
    symlink(&target_theme, extracted_theme.join("24/apps")).unwrap();

    let allowed_roots = vec![fs::canonicalize(&icon_root).unwrap()];
    materialize_absolute_theme_symlinks(
        &fs::canonicalize(directory.join("extracted")).unwrap(),
        &allowed_roots,
    )
    .unwrap();

    let copied_icon = extracted_theme.join("24/apps/app.svg");
    assert_eq!(fs::read(copied_icon).unwrap(), b"<svg/>");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn discovers_cursor_themes_before_materializing_absolute_system_links() {
    let directory = temp_dir("absolute-cursor-theme-link");
    let icon_root = directory.join("share/icons");
    let system_cursor = icon_root.join("breeze_cursors/cursors/default");
    let extracted_root = directory.join("extracted");
    let cursor_theme = extracted_root.join("FixtureCursorTheme");
    fs::create_dir_all(system_cursor.parent().unwrap()).unwrap();
    fs::create_dir_all(cursor_theme.join("cursors")).unwrap();
    fs::write(&system_cursor, b"cursor").unwrap();
    fs::write(
        cursor_theme.join("index.theme"),
        "[Icon Theme]\nName=Fixture Cursor Theme\n",
    )
    .unwrap();
    symlink(&system_cursor, cursor_theme.join("cursors/default")).unwrap();

    assert!(is_valid_theme_directory(&cursor_theme, "cursors").unwrap());
    let allowed_roots = vec![fs::canonicalize(icon_root).unwrap()];
    let canonical_extracted_root = fs::canonicalize(&extracted_root).unwrap();
    materialize_absolute_theme_symlinks(&canonical_extracted_root, &allowed_roots).unwrap();
    assert!(is_valid_theme_directory(&cursor_theme, "cursors").unwrap());

    let mut entry_count = 0;
    let mut extracted_bytes = 0;
    validate_extracted_tree(
        &canonical_extracted_root,
        &canonical_extracted_root,
        0,
        &mut entry_count,
        &mut extracted_bytes,
    )
    .unwrap();
    assert_eq!(
        fs::read(cursor_theme.join("cursors/default")).unwrap(),
        b"cursor"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rejects_absolute_theme_links_outside_installed_icon_roots() {
    let directory = temp_dir("unsafe-absolute-theme-link");
    let icon_root = directory.join("share/icons");
    let outside_target = directory.join("outside.svg");
    let extracted_theme = directory.join("extracted/Slot-Theme");
    fs::create_dir_all(&icon_root).unwrap();
    fs::create_dir_all(&extracted_theme).unwrap();
    fs::write(&outside_target, b"outside").unwrap();
    symlink(&outside_target, extracted_theme.join("icon.svg")).unwrap();

    let allowed_roots = vec![fs::canonicalize(icon_root).unwrap()];
    assert!(materialize_absolute_theme_symlinks(
        &fs::canonicalize(directory.join("extracted")).unwrap(),
        &allowed_roots,
    )
    .is_err());
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
}

#[test]
fn reports_kde_store_catalog_error_details() {
    let response = serde_json::json!({
        "status": "failed",
        "statuscode": 999,
        "message": "unknown request"
    });
    let error = parse_store_themes_response(&response).err().unwrap();
    assert_eq!(
        error.to_string(),
        "KDE Store catalog API returned status 999: unknown request"
    );
}

#[test]
fn parses_kde_store_catalog_themes() {
    let response = serde_json::json!({
        "status": "ok",
        "data": [{
            "name": "Blue Icons",
            "detailpage": "https://store.kde.org/p/12345",
            "summary": "A blue icon theme",
            "personid": "Theme Author"
        }]
    });
    let themes = parse_store_themes_response(&response).unwrap();
    assert_eq!(themes.len(), 1);
    assert_eq!(themes[0].name, "Blue Icons");
    assert_eq!(themes[0].author, "Theme Author");
    assert_eq!(themes[0].detail_url, "https://store.kde.org/p/12345");
    assert_eq!(hue_search_term("#DF8657").unwrap(), "orange");
    assert_eq!(hue_search_term("#E8C547").unwrap(), "yellow");
    assert!(hue_search_term("blue").is_err());
}

#[test]
fn compares_repository_files_and_ignores_readmes() {
    let baseline = BTreeMap::from([
        ("src/changed.rs".to_string(), "a".repeat(40)),
        ("src/removed.rs".to_string(), "b".repeat(40)),
        ("README.md".to_string(), "c".repeat(40)),
    ]);
    let remote = BTreeMap::from([
        ("src/changed.rs".to_string(), "d".repeat(40)),
        ("src/added.rs".to_string(), "e".repeat(40)),
        ("README.md".to_string(), "f".repeat(40)),
    ]);

    let changes = compare_update_files(&baseline, &remote);
    let actual: Vec<_> = changes
        .iter()
        .map(|change| (change.path.as_str(), change.status))
        .collect();
    assert_eq!(
        actual,
        vec![
            ("src/added.rs", "added"),
            ("src/changed.rs", "changed"),
            ("src/removed.rs", "removed"),
        ]
    );
    assert!(is_readme_path("README"));
    assert!(is_readme_path("docs/readme.rst"));
    assert!(!is_readme_path("src/readme_util.rs"));
}

#[test]
fn reads_baseline_and_rejects_empty_code_baseline() {
    let directory = temp_dir("update-baseline");
    let path = directory.join("baseline.txt");
    fs::write(
        &path,
        format!(
            "{}\tsrc/main.rs\n{}\tREADME.md\n",
            "a".repeat(40),
            "b".repeat(40)
        ),
    )
    .unwrap();
    let baseline = read_update_baseline(&path).unwrap();
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline["src/main.rs"], "a".repeat(40));

    fs::write(&path, format!("{}\tREADME.md\n", "b".repeat(40))).unwrap();
    assert!(read_update_baseline(&path).is_err());
    fs::remove_dir_all(directory).unwrap();
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

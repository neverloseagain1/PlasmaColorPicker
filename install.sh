#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

say() {
    if [ "$language" = "ru" ]; then
        printf '%s\n' "$2"
    else
        printf '%s\n' "$1"
    fi
}

if [ "${PCP_LANG:-}" = "en" ] || [ "${PCP_LANG:-}" = "ru" ]; then
    language=$PCP_LANG
elif [ -t 0 ]; then
    printf 'Choose language / Выберите язык:\n1) English\n2) Русский\n'
    printf '> '
    IFS= read -r language_choice || language_choice=
    case "$language_choice" in
        2|ru|RU) language=ru ;;
        *) language=en ;;
    esac
else
    language=en
fi

case "$language" in
    en) language_name=English ;;
    ru) language_name=Русский ;;
esac
say "Installer language: $language_name" "Язык установщика: $language_name"

if [ ! -f "$project_dir/metadata.json" ] || [ ! -f "$project_dir/build.sh" ]; then
    say "Run this script from the extracted PlasmaColorPicker folder." \
        "Запустите этот скрипт из распакованной папки PlasmaColorPicker."
    exit 1
fi

if [ -r /etc/os-release ]; then
    . /etc/os-release
else
    ID=
fi

packages_to_install=
missing_descriptions=

add_package() {
    package=$1
    case " $packages_to_install " in
        *" $package "*) ;;
        *) packages_to_install="${packages_to_install}${packages_to_install:+ }$package" ;;
    esac
}

add_description() {
    missing_descriptions="${missing_descriptions}${missing_descriptions:+
}$1"
}

case "${ID:-}" in
    fedora)
        package_manager=dnf
        cargo_package=cargo
        compiler_package=gcc
        plasma_package=plasma-workspace
        config_package=kf6-kconfig
        package_package=kf6-kpackage
        extractor_package=unar
        ;;
    debian|ubuntu|linuxmint|pop)
        package_manager=apt
        cargo_package=cargo
        compiler_package=build-essential
        plasma_package=plasma-workspace
        config_package=libkf6config-bin
        package_package=libkf6package6
        extractor_package=unar
        ;;
    arch|manjaro|endeavouros)
        package_manager=pacman
        cargo_package=rust
        compiler_package=gcc
        plasma_package=plasma-workspace
        config_package=kconfig
        package_package=kpackage
        extractor_package=unar
        ;;
    opensuse*|sles)
        package_manager=zypper
        cargo_package=cargo
        compiler_package=gcc
        plasma_package=plasma6-workspace
        config_package=kf6-kconfig-tools
        package_package=kf6-kpackage
        extractor_package=unar
        ;;
    *)
        package_manager=
        cargo_package=
        compiler_package=
        plasma_package=
        config_package=
        package_package=
        extractor_package=
        ;;
esac

if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    if [ "$language" = "ru" ]; then
        add_description 'Rust и Cargo — для сборки помощника'
    else
        add_description 'Rust and Cargo — to build the helper'
    fi
    [ -z "$cargo_package" ] || add_package "$cargo_package"
fi

if ! command -v cc >/dev/null 2>&1; then
    if [ "$language" = "ru" ]; then
        add_description 'Компилятор C — для сборки части помощника'
    else
        add_description 'A C compiler — to build part of the helper'
    fi
    [ -z "$compiler_package" ] || add_package "$compiler_package"
fi

if ! command -v kpackagetool6 >/dev/null 2>&1 \
    || ! command -v plasma-apply-colorscheme >/dev/null 2>&1 \
    || ! command -v plasma-apply-desktoptheme >/dev/null 2>&1; then
    if [ "$language" = "ru" ]; then
        add_description 'Утилиты KDE Plasma 6 — для установки виджета и применения цветов'
    else
        add_description 'KDE Plasma 6 tools — to install the widget and apply colors'
    fi
    [ -z "$plasma_package" ] || add_package "$plasma_package"
fi

if ! command -v kpackagetool6 >/dev/null 2>&1; then
    [ -z "$package_package" ] || add_package "$package_package"
fi

if ! command -v kwriteconfig6 >/dev/null 2>&1; then
    if [ "$language" = "ru" ]; then
        add_description 'Утилита настройки KDE — для сохранения выбранных цветов'
    else
        add_description 'KDE configuration tool — to save the selected colors'
    fi
    [ -z "$config_package" ] || add_package "$config_package"
fi

if ! command -v unar >/dev/null 2>&1 && ! command -v 7z >/dev/null 2>&1; then
    if [ "$language" = "ru" ]; then
        add_description 'Распаковщик unar — для установки архивов тем значков и курсоров'
    else
        add_description 'unar archive tool — to install icon and cursor theme archives'
    fi
    [ -z "$extractor_package" ] || add_package "$extractor_package"
fi

if [ -n "$missing_descriptions" ]; then
    say "Missing dependencies:" "Не найдены необходимые программы:"
    printf '%s\n' "$missing_descriptions"

    if [ -z "$package_manager" ] || ! command -v "$package_manager" >/dev/null 2>&1; then
        say "Automatic installation is not available for this Linux distribution. Install the listed programs from its official repositories, then run this script again." \
            "Автоматическая установка для этого дистрибутива недоступна. Установите перечисленные программы из официальных репозиториев и запустите скрипт снова."
        exit 1
    fi

    if [ -z "$packages_to_install" ]; then
        say "The missing programs could not be matched to packages for this distribution. Please install them from its official repositories." \
            "Не удалось определить пакеты для этого дистрибутива. Установите программы из его официальных репозиториев."
        exit 1
    fi

    say "The following official system packages will be installed:" \
        "Будут установлены следующие системные пакеты из официальных репозиториев:"
    set -f
    set -- $packages_to_install
    set +f
    for package do
        printf '  - %s\n' "$package"
    done
    if [ -t 0 ]; then
        say "Install them now with sudo? [y/N]" "Установить их сейчас через sudo? [д/Н]"
        printf '> '
        IFS= read -r install_answer || install_answer=
    else
        install_answer=${PCP_INSTALL_DEPENDENCIES:-no}
    fi

    case "$install_answer" in
        y|Y|yes|YES|д|Д|да|ДА) ;;
        *)
            say "No packages were installed. Install the missing programs and run this script again." \
                "Пакеты не устанавливались. Установите недостающие программы и запустите скрипт снова."
            exit 1
            ;;
    esac

    if ! command -v sudo >/dev/null 2>&1; then
        say "sudo is not available. Install it or install the listed packages manually using your system package manager." \
            "Команда sudo не найдена. Установите её или установите перечисленные пакеты вручную через системный менеджер пакетов."
        exit 1
    fi

    set -f
    set -- $packages_to_install
    case "$package_manager" in
        dnf) sudo dnf install --assumeyes "$@" ;;
        apt) sudo apt-get install --yes "$@" ;;
        pacman) sudo pacman -S --needed --noconfirm "$@" ;;
        zypper) sudo zypper --non-interactive install "$@" ;;
    esac
    set +f

    if ! command -v cargo >/dev/null 2>&1 \
        || ! command -v rustc >/dev/null 2>&1 \
        || ! command -v cc >/dev/null 2>&1 \
        || ! command -v kpackagetool6 >/dev/null 2>&1 \
        || ! command -v plasma-apply-colorscheme >/dev/null 2>&1 \
        || ! command -v plasma-apply-desktoptheme >/dev/null 2>&1 \
        || ! command -v kwriteconfig6 >/dev/null 2>&1 \
        || { ! command -v unar >/dev/null 2>&1 && ! command -v 7z >/dev/null 2>&1; }; then
        say "Some required programs are still missing after installation. Check the package manager output and try again." \
            "После установки часть программ всё ещё не найдена. Проверьте сообщение менеджера пакетов и повторите попытку."
        exit 1
    fi
fi

say "Building the widget helper…" "Сборка помощника виджета…"
"$project_dir/build.sh"

staging_dir=$(mktemp -d "${TMPDIR:-/tmp}/plasmacolorpicker-package.XXXXXX")
case "$staging_dir" in
    "${TMPDIR:-/tmp}"/plasmacolorpicker-package.*) ;;
    *)
        say "Could not create a safe temporary package folder." \
            "Не удалось создать временную папку для установки."
        exit 1
        ;;
esac
trap 'rm -r -- "$staging_dir"' EXIT HUP INT TERM

cp "$project_dir/metadata.json" "$staging_dir/metadata.json"
cp -R "$project_dir/contents" "$staging_dir/contents"

if kpackagetool6 --type Plasma/Applet --list | grep -Fxq 'org.example.wallpaperpalette'; then
    say "Updating the existing widget…" "Обновление установленного виджета…"
    if ! kpackagetool6 --type Plasma/Applet --upgrade "$staging_dir" >"$staging_dir/install-output.txt" 2>&1; then
        cat "$staging_dir/install-output.txt" >&2
        say "The widget could not be updated." "Не удалось обновить виджет." >&2
        exit 1
    fi
else
    say "Installing the widget for your account…" "Установка виджета для вашей учётной записи…"
    if ! kpackagetool6 --type Plasma/Applet --install "$staging_dir" >"$staging_dir/install-output.txt" 2>&1; then
        cat "$staging_dir/install-output.txt" >&2
        say "The widget could not be installed." "Не удалось установить виджет." >&2
        exit 1
    fi
fi

icon_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}/icons/hicolor/512x512/apps
mkdir -p "$icon_dir"
cp "$project_dir/contents/images/logo.png" "$icon_dir/org.example.plasmacolorpicker.png"
if command -v kbuildsycoca6 >/dev/null 2>&1; then
    if ! kbuildsycoca6 --noincremental >"$staging_dir/cache-output.txt" 2>&1; then
        cat "$staging_dir/cache-output.txt" >&2
        say "The icon cache could not be refreshed." "Не удалось обновить кэш значков." >&2
        exit 1
    fi
fi

say "Installation is complete. Add PlasmaColorPicker from the widget list. Use the EN/RU button to change the widget language." \
    "Установка завершена. Добавьте PlasmaColorPicker через список виджетов. Язык виджета переключается кнопкой EN/RU."

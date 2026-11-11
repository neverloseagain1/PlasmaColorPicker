#!/bin/sh
set -eu

case "${PCP_LANG:-${LC_MESSAGES:-${LANG:-en}}}" in
    ru|ru_*|ru.*) language=ru ;;
    *) language=en ;;
esac

say() {
    if [ "$language" = "ru" ]; then
        printf '%s\n' "$2"
    else
        printf '%s\n' "$1"
    fi
}

if ! command -v kpackagetool6 >/dev/null 2>&1; then
    say "KDE's kpackagetool6 was not found. The widget was not removed." \
        "Не найдена утилита KDE kpackagetool6. Виджет не удалён."
    exit 1
fi

if kpackagetool6 --type Plasma/Applet --list | grep -Fxq 'org.example.wallpaperpalette'; then
    kpackagetool6 --type Plasma/Applet --remove org.example.wallpaperpalette
else
    say "PlasmaColorPicker is not installed for this account." \
        "PlasmaColorPicker не установлен для этой учётной записи."
fi

data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
icon_file=$data_home/icons/hicolor/512x512/apps/org.example.plasmacolorpicker.png
if [ -f "$icon_file" ]; then
    rm -- "$icon_file"
fi

if command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6 --noincremental >/dev/null
fi

say "The widget and its icon were removed. Your saved palettes and installed themes were left unchanged." \
    "Виджет и его значок удалены. Сохранённые палитры и установленные темы не изменены."

# PlasmaColorPicker (PCP) — Early Access

[Русский](README.ru.md) | **English**

PlasmaColorPicker is a widget for KDE Plasma 6. It can:

- look at your current wallpaper and suggest up to three matching color palettes;
- apply a palette to Plasma;
- find matching icon and cursor themes in KDE Store;
- install an icon or cursor theme from an archive.

The widget acts only when you press a button. It does not watch your wallpaper
or send it anywhere. Internet access is used only when you search KDE Store.

## Install
Run:

   ```sh
   git clone https://github.com/neverloseagain1/PlasmaColorPicker
   cd PlasmaColorPicker
   ./install.sh
   ```

The installer asks which language to use, checks the programs it needs, builds
the widget, and installs it for your account. If something is missing, it shows
the packages it needs and asks before installing them through your Linux
system's official package manager. You may be asked for your password by
`sudo`. The installer does not install anything if you say no.

Automatic dependency installation is prepared for Fedora, Debian and Ubuntu,
Arch Linux, and openSUSE. On other distributions, install the missing programs
from their official repositories and run the installer again. The required
programs are Rust/Cargo, a C compiler, KDE Plasma 6 command-line tools, a KDE
settings tool, and `unar` or `7z` for theme archives.

After installation, right-click your desktop or panel, choose **Add Widgets**,
find **PlasmaColorPicker (PCP)**, and add it.

## Use

Click the PCP icon to open the widget. Use **EN/RU** at the top to change the
widget language. The installer's language choice affects only installer
messages.

### Palettes

On **Palettes**, choose **Analyze wallpaper**. PCP shows color suggestions and
what each color is for. Select **Apply palette** to save and use one. PCP keeps
the original Plasma theme unchanged and makes its own copy for the new colors.

**Delete palettes** removes palettes made by PCP and restores the previous
appearance.

### Find and install themes

On **Icon themes** or **Cursor themes**, choose a palette and select **Find
icons** or **Find cursors**. Choose a result to open it in KDE Store.

On **Install**, choose whether the archive contains icons or cursors. Drop the
archive into the box or choose a file. PCP checks it and installs matching
themes for your account. It does not replace an existing theme. If it cannot
install an archive, it explains what to do next.

To use an installed theme, open **System Settings → Colors & Themes → Icons**
or **Cursors** and select it. If that page was already open, close and reopen
it.

## Remove

To remove the widget and its icon from your account, run:

```sh
./uninstall.sh
```

This leaves palettes you previously applied and themes you installed unchanged.

## Notes

Some wallpaper types, such as slideshows, do not provide an image that PCP can
read. Theme descriptions shown in search results come from KDE Store and may be
written in another language.

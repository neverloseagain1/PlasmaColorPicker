// Main widget UI: coordinates helper commands, tabs, status messages, and updates.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.plasma.plasma5support as Plasma5Support
import org.kde.plasma.plasmoid

PlasmoidItem {
    id: root

    property bool keepOpen: false
    hideOnWindowDeactivate: !keepOpen

    implicitWidth: 400
    implicitHeight: 620

    property var palettes: []
    property var iconThemes: []
    property var cursorThemes: []
    property bool russian: Plasmoid.configuration.language !== "en"
    property string statusText: text("Press Analyze to read the current wallpaper.", "Нажмите «Анализировать обои», чтобы прочитать текущие обои.")
    property bool statusIsError: false
    property string diagnosticText: ""
    property string storeStatus: ""
    property bool storeStatusIsError: false
    property string cursorStatus: ""
    property bool cursorStatusIsError: false
    property string iconStoreSearchUrl: ""
    property string cursorStoreSearchUrl: ""
    property string themeInstallStatus: ""
    property bool themeInstallFailed: false
    property int selectedThemeType: 0
    property int analysisTypeIndex: 0
    property string paletteAnalysisType: "auto"
    property int selectedTabIndex: 0
    property bool busy: false
    property string activeAction: ""
    property string activeCommand: ""
    property bool activeSearch: false
    property string searchAction: ""
    property int searchElapsedSeconds: 0
    property string timedOutSearchAction: ""
    property string helperPath: decodeURIComponent(Qt.resolvedUrl("../bin/plasmacolorpicker-helper").toString().replace(/^file:\/\//, ""))
    property string updateBaselinePath: decodeURIComponent(Qt.resolvedUrl("../update-baseline.txt").toString().replace(/^file:\/\//, ""))
    property bool updateCheckAttempted: false
    property bool updateAvailable: false
    property bool updateStatusIsError: false
    property string updateStatus: text(
        "The update check runs once when you first open PCP in this session.",
        "Проверка обновлений выполняется один раз при первом открытии PCP за сеанс."
    )
    onExpandedChanged: {
        if (root.expanded && !updateCheckAttempted) {
            updateCheckTimer.start()
        }
    }

    onRussianChanged: {
        statusText = text(
            "Press Analyze to read the current wallpaper.",
            "Нажмите «Анализировать обои», чтобы прочитать текущие обои."
        )
        statusIsError = false
        storeStatus = ""
        storeStatusIsError = false
        cursorStatus = ""
        cursorStatusIsError = false
        iconStoreSearchUrl = ""
        cursorStoreSearchUrl = ""
        themeInstallStatus = ""
        themeInstallFailed = false
        diagnosticText = ""
    }

    SystemPalette {
        id: systemPalette
    }

    function shellQuote(value) {
        return "'" + value.replace(/'/g, "'\\''") + "'"
    }

    function runHelper(args) {
        if (busy) {
            return
        }
        busy = true
        activeAction = String(args[0] || "")
        const command = [helperPath].concat(args).map(argument => shellQuote(String(argument))).join(" ")
        activeCommand = command
        if (activeAction === "icons" || activeAction === "cursors") {
            activeSearch = true
            searchAction = activeAction
            searchElapsedSeconds = 0
            timedOutSearchAction = ""
            searchTimer.restart()
        }
        commandSource.connectSource(command)
    }

    function startUpdateCheck(force) {
        if ((force !== true && updateCheckAttempted) || busy || !expanded) {
            return
        }
        updateCheckTimer.stop()
        updateCheckAttempted = true
        updateStatus = text("Checking GitHub for code updates…", "Проверка обновлений кода на GitHub…")
        updateStatusIsError = false
        updateAvailable = false
        runHelper(["check-updates", updateBaselinePath])
    }

    function stopSearch(timedOut) {
        if (!activeSearch) {
            return
        }

        const action = searchAction
        searchTimer.stop()
        activeSearch = false
        searchAction = ""
        searchElapsedSeconds = 0
        if (activeCommand.length > 0) {
            commandSource.disconnectSource(activeCommand)
        }
        activeCommand = ""
        activeAction = ""
        busy = false

        if (action === "icons") {
            iconThemes = []
            storeStatus = ""
            storeStatusIsError = false
        } else {
            cursorThemes = []
            cursorStatus = ""
            cursorStatusIsError = false
        }
        timedOutSearchAction = timedOut ? action : ""
        if (!timedOut) {
            const message = text("Search cancelled.", "Поиск отменён.")
            if (action === "icons") {
                storeStatus = message
                storeStatusIsError = false
            } else {
                cursorStatus = message
                cursorStatusIsError = false
            }
        }
    }

    function text(english, russian) {
        return root.russian ? russian : english
    }

    function analysisErrorMessage(details) {
        const message = String(details)
        if (message.indexOf("requires a GIF or animated WebP") >= 0) {
            return text(
                "The active wallpaper is not an animated GIF or WebP. Choose Automatic or another source type.",
                "Активные обои — не анимированный GIF/WebP. Выберите автоматический режим или другой тип источника."
            )
        }
        if (message.indexOf("requires the active wallpaper source to be a video") >= 0) {
            return text(
                "The active wallpaper source is not a video. Choose Automatic or another source type.",
                "Активные обои — не видео. Выберите автоматический режим или другой тип источника."
            )
        }
        if (message.indexOf("Select Auto, Animated image, or Video") >= 0) {
            return text(
                "This wallpaper is animated. Choose Automatic, Animated image, or Video analysis.",
                "Эти обои анимированные. Выберите автоматический режим, GIF/WebP или видео."
            )
        }
        if (message.indexOf("Slideshow mode requires") >= 0) {
            return text(
                "Slideshow mode works with Plasma's built-in Slideshow wallpaper only.",
                "Режим слайд-шоу работает только со встроенным плагином обоев «Слайд-шоу» в Plasma."
            )
        }
        if (message.indexOf("requires ffprobe from FFmpeg") >= 0
                || message.indexOf("requires FFmpeg") >= 0) {
            return text(
                "Video analysis needs FFmpeg (ffmpeg and ffprobe). Install the ffmpeg package from your distribution's repositories, then try again.",
                "Для анализа видео нужны FFmpeg (программы ffmpeg и ffprobe). Установите пакет ffmpeg из репозитория своего дистрибутива и повторите попытку."
            )
        }
        return message
    }

    function resetTabStatus(index) {
        if (index === 0) {
            statusText = text(
                "Press Analyze to read the current wallpaper.",
                "Нажмите «Анализировать обои», чтобы прочитать текущие обои."
            )
            statusIsError = false
        } else if (index === 1) {
            storeStatus = ""
            storeStatusIsError = false
        } else if (index === 2) {
            cursorStatus = ""
            cursorStatusIsError = false
        } else if (index === 3) {
            themeInstallStatus = ""
            themeInstallFailed = false
        }
    }

    function paletteName(index) {
        const number = String(index + 1).padStart(2, "0")
        return root.text("PlasmaColorPicker " + number, "Палитра PCP " + number)
    }

    function paletteColorRoles(palette) {
        return [
            { label: root.text("Panel", "Панель"), color: palette.panel },
            { label: root.text("Window background", "Фон окон"), color: palette.background },
            { label: root.text("Lists and cards", "Списки и карточки"), color: palette.surface },
            { label: root.text("Selection", "Выделение"), color: palette.selection },
            { label: root.text("Accent and links", "Акцент и ссылки"), color: palette.accent },
            { label: root.text("Text", "Текст"), color: palette.foreground }
        ]
    }

    function mixPalettes() {
        const mixed = root.palettes.slice()
        for (let index = mixed.length - 1; index > 0; index--) {
            const swapIndex = Math.floor(Math.random() * (index + 1))
            const current = mixed[index]
            mixed[index] = mixed[swapIndex]
            mixed[swapIndex] = current
        }
        if (mixed.length > 1 && mixed.every((palette, index) => palette.index === root.palettes[index].index)) {
            mixed.push(mixed.shift())
        }
        root.palettes = mixed
    }

    function applyPalette(palette) {
        if (!Number.isInteger(palette.index) || palette.index < 0 || palette.index > 2) {
            statusText = text("The selected palette has an invalid index.", "У выбранной палитры некорректный номер.")
            statusIsError = true
            return
        }
        runHelper(["apply", palette.index, paletteAnalysisType])
    }

    function installTheme(archive) {
        const archivePath = String(archive)
        if (archivePath.length === 0) {
            showThemeInstallError(text("No archive was selected.", "Архив не выбран."))
            return
        }
        const themeType = selectedThemeType === 0 ? "icons" : "cursors"
        themeInstallFailed = false
        themeInstallStatus = text("Checking the archive…", "Проверка архива…")
        runHelper(["install-theme", themeType, archivePath])
    }

    function showThemeInstallError(details) {
        const themeType = selectedThemeType === 0
            ? text("icon theme", "тему значков")
            : text("cursor theme", "тему курсоров")
        themeInstallFailed = true
        themeInstallStatus = text(
            "Unfortunately, the %1 could not be installed. Please follow the instructions below.\nDetails: %2",
            "К сожалению, не удалось установить %1. Пожалуйста, следуйте инструкции ниже.\nПричина: %2"
        ).replace("%1", themeType).replace("%2", details)
    }

    function openStoreItem(url) {
        if (/^https:\/\/store\.kde\.org\/p\/[0-9]+$/.test(url)) {
            Qt.openUrlExternally(url)
        } else {
            const message = text("KDE Store returned an invalid item link.", "KDE Store вернул некорректную ссылку.")
            if (selectedTabIndex === 2) {
                cursorStatus = message
                cursorStatusIsError = true
            } else {
                storeStatus = message
                storeStatusIsError = true
            }
        }
    }

    function openStoreSearch(url) {
        if (/^https:\/\/store\.kde\.org\/browse\?cat=(132|107)&search=(red|orange|yellow|green|teal|blue|purple|pink|dark)$/.test(url)) {
            Qt.openUrlExternally(url)
        } else {
            const message = text("KDE Store returned an invalid search link.", "KDE Store вернул некорректную ссылку для поиска.")
            if (selectedTabIndex === 2) {
                cursorStatus = message
                cursorStatusIsError = true
            } else {
                storeStatus = message
                storeStatusIsError = true
            }
        }
    }

    function removePalettes() {
        diagnosticText = ""
        storeStatus = text("Removing widget palettes…", "Удаление палитр виджета…")
        storeStatusIsError = false
        cursorStatus = storeStatus
        cursorStatusIsError = false
        statusText = storeStatus
        statusIsError = false
        runHelper(["remove"])
    }

    function diagnosticLabel(id) {
        const labels = {
            "wallpaper": text("Wallpaper settings", "Настройки обоев"),
            "image": text("Wallpaper media source", "Медиаисточник обоев"),
            "apply-command": text("KDE color-scheme command", "Команда цветовых схем KDE"),
            "desktop-theme-command": text("Plasma theme command", "Команда темы Plasma"),
            "archive-command": text("Theme archive extractor", "Распаковщик архивов тем"),
            "desktop-theme": text("Active Plasma theme", "Активная тема Plasma"),
            "output-directory": text("Color-scheme folder", "Папка цветовых схем"),
            "desktop-theme-directory": text("Plasma theme folder", "Папка тем Plasma")
        }
        return labels[id] || id
    }

    function diagnosticResult(checks) {
        return checks.map(check => {
            const state = check.ok
                ? text("✓ Available", "✓ Доступно")
                : text("✗ Problem", "✗ Проблема")
            let details = String(check.details)
            if (check.id === "image" && check.ok) {
                details = details.replace(/^Decoded /, text("Decoded: ", "Изображение прочитано: "))
            } else if (!check.ok) {
                const translatedDetails = {
                    "plasma-apply-colorscheme was not found in PATH":
                        "Команда plasma-apply-colorscheme не найдена.",
                    "plasma-apply-desktoptheme was not found in PATH":
                        "Команда plasma-apply-desktoptheme не найдена.",
                    "Neither unar nor 7z was found in PATH; archive themes can be installed manually":
                        "Не найдены unar и 7z. Темы можно установить вручную.",
                    "No active Plasma desktop theme was found":
                        "Не удалось определить активную тему Plasma."
                }
                if (details.indexOf("Video analysis requires") === 0) {
                    details = root.analysisErrorMessage(details)
                }
                if (root.russian && translatedDetails[details]) {
                    details = translatedDetails[details]
                }
                details = text("Details: ", "Подробности: ") + details
            }
            return state + " — " + diagnosticLabel(check.id) + ": " + details
        }).join("\n")
    }

    compactRepresentation: QQC2.ToolButton {
        id: compactButton
        implicitWidth: 32
        implicitHeight: 32
        Accessible.name: "PlasmaColorPicker"
        background: Item {}
        contentItem: Item {
            Image {
                source: Qt.resolvedUrl("../images/logo.png")
                fillMode: Image.PreserveAspectFit
                width: 24
                height: 24
                opacity: compactButton.hovered ? 1 : 0.82
                anchors.centerIn: parent
                anchors.horizontalCenterOffset: 1
                Behavior on opacity {
                    NumberAnimation { duration: 120 }
                }
            }
        }
        onClicked: root.expanded = !root.expanded
    }

    fullRepresentation: Item {
        id: panel
        Layout.minimumWidth: 400
        Layout.minimumHeight: 620
        implicitWidth: 400
        implicitHeight: 620

        Rectangle {
            anchors.fill: parent
            color: systemPalette.window

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 16
                spacing: 12

                RowLayout {
                    Layout.fillWidth: true

                    QQC2.Label {
                        Layout.fillWidth: true
                        text: root.text("PlasmaColorPicker (PCP)", "PlasmaColorPicker (PCP)")
                        font.bold: true
                        font.pointSize: 14
                        elide: Text.ElideRight
                    }

                    QQC2.ToolButton {
                        id: keepOpenButton
                        checkable: true
                        checked: root.keepOpen
                        icon.source: Qt.resolvedUrl("../images/pin.svg")
                        icon.color: systemPalette.windowText
                        Accessible.name: root.text("Keep widget open", "Не закрывать виджет")
                        onToggled: root.keepOpen = checked
                        QQC2.ToolTip.text: checked
                            ? root.text("Widget stays open when it loses focus", "Виджет не закроется при переключении в другое окно")
                            : root.text("Widget closes when it loses focus", "Виджет закроется при переключении в другое окно")
                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.delay: 500
                    }

                    QQC2.ToolButton {
                        text: root.russian ? "EN" : "RU"
                        Accessible.name: root.russian ? "Переключить на английский" : "Switch to Russian"
                        onClicked: Plasmoid.configuration.language = root.russian ? "en" : "ru"
                    }

                    QQC2.ToolButton {
                        text: root.text("Delete palettes", "Удалить палитры")
                        Accessible.name: root.text("Delete widget palettes", "Удалить палитры виджета")
                        enabled: !root.busy
                        onClicked: root.removePalettes()
                    }

                    QQC2.ToolButton {
                        text: root.text("Close", "Закрыть")
                        onClicked: root.expanded = false
                    }
                }

                QQC2.TabBar {
                    id: mainTabs
                    Layout.fillWidth: true
                    Component.onCompleted: {
                        root.selectedTabIndex = currentIndex
                        root.resetTabStatus(currentIndex)
                    }
                    onCurrentIndexChanged: {
                        root.selectedTabIndex = currentIndex
                        root.resetTabStatus(currentIndex)
                    }

                    QQC2.TabButton {
                        text: root.text("Palettes", "Палитры")
                    }

                    QQC2.TabButton {
                        text: root.text("Icon themes", "Темы значков")
                    }

                    QQC2.TabButton {
                        text: root.text("Cursor themes", "Темы курсоров")
                    }

                    QQC2.TabButton {
                        text: root.text("Install", "Установить")
                    }

                    QQC2.TabButton {
                        text: root.text("Update", "Обновление")
                    }
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    visible: mainTabs.currentIndex === 0 && !root.statusIsError
                    text: root.statusText
                    wrapMode: Text.Wrap
                    color: systemPalette.windowText
                }

                CopyableText {
                    Layout.fillWidth: true
                    visible: mainTabs.currentIndex === 0 && root.statusIsError
                    text: root.statusText
                    copyButtonText: root.text("Copy error", "Копировать ошибку")
                    maximumHeight: 140
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    visible: mainTabs.currentIndex === 0
                    text: root.text(
                        "Automatic mode detects the active wallpaper source. GIF/WebP and video modes sample multiple frames; video needs FFmpeg.",
                        "Автоматический режим определяет источник активных обоев. Для GIF/WebP и видео анализируются несколько кадров; видео требует FFmpeg."
                    )
                    wrapMode: Text.Wrap
                    font.pointSize: 9
                    color: systemPalette.windowText
                }

                RowLayout {
                    Layout.fillWidth: true
                    visible: mainTabs.currentIndex === 0
                    spacing: 8

                    QQC2.ComboBox {
                        Layout.fillWidth: true
                        model: [
                            root.text("Automatic", "Автоматически"),
                            root.text("Photo / image", "Фото / изображение"),
                            root.text("Animation (GIF/WebP)", "Анимация (GIF/WebP)"),
                            root.text("Video", "Видео"),
                            root.text("Slideshow", "Слайд-шоу")
                        ]
                        currentIndex: root.analysisTypeIndex
                        onActivated: {
                            root.analysisTypeIndex = currentIndex
                            root.palettes = []
                            root.diagnosticText = ""
                            root.statusIsError = false
                            root.statusText = root.text(
                                "Select Analyze wallpaper to create palettes for this source type.",
                                "Нажмите «Анализировать обои», чтобы создать палитры для этого типа источника."
                            )
                        }
                        enabled: !root.busy
                    }

                    QQC2.Button {
                        Layout.fillWidth: true
                        text: root.busy
                            ? root.text("Working…", "Выполняется…")
                            : root.text("Analyze wallpaper", "Анализировать обои")
                        enabled: !root.busy
                        onClicked: {
                            root.paletteAnalysisType = [
                                "auto", "image", "gif", "video", "slideshow"
                            ][root.analysisTypeIndex]
                            root.runHelper(["analyze", root.paletteAnalysisType])
                        }
                    }

                    QQC2.Button {
                        visible: Plasmoid.configuration.showDiagnostics
                        Layout.fillWidth: visible
                        text: root.text("Check", "Проверка")
                        enabled: !root.busy
                        onClicked: root.runHelper(["diagnose"])
                    }
                }

                CopyableText {
                    Layout.fillWidth: true
                    visible: mainTabs.currentIndex === 0
                        && root.diagnosticText.length > 0
                        && root.diagnosticText.indexOf("✗") >= 0
                    text: root.diagnosticText
                    copyButtonText: root.text("Copy diagnostic", "Копировать диагностику")
                    maximumHeight: 180
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    visible: mainTabs.currentIndex === 0
                        && root.diagnosticText.length > 0
                        && root.diagnosticText.indexOf("✗") < 0
                    text: root.diagnosticText
                    wrapMode: Text.Wrap
                    color: systemPalette.windowText
                    font.pointSize: 9
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    visible: mainTabs.currentIndex === 0
                    text: root.text(
                        "Color guide: panel, window background, lists/cards, selection, accent/links, text.",
                        "Подписи цветов: панель, фон окон, списки/карточки, выделение, акцент/ссылки, текст."
                    )
                    wrapMode: Text.Wrap
                    font.pointSize: 9
                    color: systemPalette.windowText
                }

                ListView {
                    id: paletteList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: mainTabs.currentIndex === 0
                    clip: true
                    spacing: 8
                    model: root.palettes

                    delegate: Rectangle {
                        required property var modelData
                        width: paletteList.width
                        height: 240
                        radius: 8
                        color: systemPalette.alternateBase

                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: 10
                            spacing: 6

                            QQC2.Label {
                                Layout.fillWidth: true
                                text: root.paletteName(modelData.index)
                                font.bold: true
                            }

                            Repeater {
                                model: root.paletteColorRoles(modelData)

                                delegate: RowLayout {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 8

                                    Rectangle {
                                        Layout.preferredWidth: 24
                                        Layout.preferredHeight: 18
                                        radius: 3
                                        color: modelData.color
                                        border.color: systemPalette.windowText
                                        border.width: 1
                                    }

                                    QQC2.Label {
                                        Layout.fillWidth: true
                                        text: modelData.label
                                        elide: Text.ElideRight
                                        font.pointSize: 9
                                    }

                                    QQC2.Label {
                                        text: modelData.color
                                        font.family: "monospace"
                                        font.pointSize: 9
                                    }
                                }
                            }

                            QQC2.Button {
                                Layout.alignment: Qt.AlignRight
                                text: root.text("Apply palette", "Применить палитру")
                                enabled: !root.busy
                                onClicked: root.applyPalette(modelData)
                            }
                        }

                    }
                }

                QQC2.Button {
                    Layout.fillWidth: true
                    visible: mainTabs.currentIndex === 0 && root.palettes.length > 1
                    enabled: visible && !root.busy
                    text: root.text("Mix colors", "Смешать цвета")
                    onClicked: root.mixPalettes()
                }

                ThemeSearchPanel {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: mainTabs.currentIndex === 1
                    russian: root.russian
                    hasPalettes: root.palettes.length > 0
                    iconThemes: true
                    themes: root.iconThemes
                    statusText: root.storeStatus
                    statusIsError: root.storeStatusIsError
                    storeSearchUrl: root.iconStoreSearchUrl
                    busy: root.busy
                    searching: root.activeSearch && root.searchAction === "icons"
                    searchElapsedSeconds: root.searchElapsedSeconds
                    timedOut: root.timedOutSearchAction === "icons"
                    onSearchRequested: function(paletteIndex) {
                        if (paletteIndex < 0) {
                            root.stopSearch(false)
                            return
                        }
                        root.iconThemes = []
                        root.iconStoreSearchUrl = ""
                        root.storeStatus = root.text("Searching KDE Store…", "Поиск в KDE Store…")
                        root.storeStatusIsError = false
                        root.runHelper(["icons", paletteIndex, root.paletteAnalysisType])
                    }
                    onOpenStoreItem: function(url) {
                        root.openStoreItem(url)
                    }
                    onOpenStoreSearch: function(url) {
                        root.openStoreSearch(url)
                    }
                }

                ThemeSearchPanel {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: mainTabs.currentIndex === 2
                    russian: root.russian
                    hasPalettes: root.palettes.length > 0
                    iconThemes: false
                    themes: root.cursorThemes
                    statusText: root.cursorStatus
                    statusIsError: root.cursorStatusIsError
                    storeSearchUrl: root.cursorStoreSearchUrl
                    busy: root.busy
                    searching: root.activeSearch && root.searchAction === "cursors"
                    searchElapsedSeconds: root.searchElapsedSeconds
                    timedOut: root.timedOutSearchAction === "cursors"
                    onSearchRequested: function(paletteIndex) {
                        if (paletteIndex < 0) {
                            root.stopSearch(false)
                            return
                        }
                        root.cursorThemes = []
                        root.cursorStoreSearchUrl = ""
                        root.cursorStatus = root.text("Searching KDE Store…", "Поиск в KDE Store…")
                        root.cursorStatusIsError = false
                        root.runHelper(["cursors", paletteIndex, root.paletteAnalysisType])
                    }
                    onOpenStoreItem: function(url) {
                        root.openStoreItem(url)
                    }
                    onOpenStoreSearch: function(url) {
                        root.openStoreSearch(url)
                    }
                }

                QQC2.ScrollView {
                    id: installScrollView
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: mainTabs.currentIndex === 3
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: installScrollView.availableWidth
                        spacing: 10

                        QQC2.Label {
                            Layout.fillWidth: true
                            text: root.text(
                                "Choose what the archive contains, then drop it here or browse for a file. PCP tries unar first and tries 7z if unar cannot extract it. Common formats include ZIP, 7z, RAR, TAR.GZ, TAR.XZ and TAR.ZST. Some themes with relative links need unar.",
                                "Выберите тип темы и перетащите архив сюда или укажите файл. Сначала PCP пробует unar, а если он не может распаковать архив — 7z. Поддерживаются распространённые форматы ZIP, 7z, RAR, TAR.GZ, TAR.XZ и TAR.ZST. Для некоторых тем с относительными ссылками нужен unar."
                            )
                            wrapMode: Text.Wrap
                            color: systemPalette.windowText
                        }

                        QQC2.ComboBox {
                            id: themeTypeSelector
                            Layout.fillWidth: true
                            model: [
                                root.text("Icon theme", "Тема значков"),
                                root.text("Cursor theme", "Тема курсоров")
                            ]
                            currentIndex: root.selectedThemeType
                            onActivated: root.selectedThemeType = currentIndex
                            enabled: !root.busy
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 150
                            color: systemPalette.alternateBase
                            radius: 8
                            border.color: dropArea.containsDrag ? systemPalette.highlight : systemPalette.mid
                            border.width: dropArea.containsDrag ? 2 : 1

                            QQC2.Label {
                                anchors.fill: parent
                                anchors.margins: 16
                                text: root.text(
                                    "Drop a theme archive here",
                                    "Перетащите архив темы сюда"
                                )
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                                wrapMode: Text.Wrap
                            }

                            DropArea {
                                id: dropArea
                                anchors.fill: parent
                                enabled: !root.busy

                                onDropped: function(drop) {
                                    if (drop.hasUrls && drop.urls.length > 0) {
                                        drop.accepted = true
                                        root.installTheme(drop.urls[0])
                                    } else {
                                        drop.accepted = false
                                        root.showThemeInstallError(root.text(
                                            "Drop a local archive file.",
                                            "Перетащите локальный файл архива."
                                        ))
                                    }
                                }
                            }
                        }

                        QQC2.Button {
                            Layout.fillWidth: true
                            text: root.busy && root.activeAction === "install-theme"
                                ? root.text("Installing…", "Установка…")
                                : root.text("Choose archive…", "Выбрать архив…")
                            enabled: !root.busy
                            onClicked: archiveDialog.open()
                        }

                        CopyableText {
                            Layout.fillWidth: true
                            visible: root.themeInstallStatus.length > 0
                            showCopyButton: root.themeInstallFailed
                            text: root.themeInstallStatus
                            copyButtonText: root.text("Copy error", "Копировать ошибку")
                        }

                        CopyableText {
                            Layout.fillWidth: true
                            visible: root.themeInstallFailed
                            text: root.text(
                                "Manual installation:\n1. In your file manager, extract the downloaded archive.\n2. Open the extracted folders until you find the theme folder containing index.theme. Use that folder itself, not the outer archive folder.\n3. Copy the command below. Replace only /path/to/ThemeFolder with the full path to the theme folder. Keep the quotes; they make paths with spaces work. Example: \"/home/alex/Downloads/My Theme\".\n4. Open Konsole, paste the edited command, and press Enter. Do not add sudo. If a folder with the same theme name already exists in the destination, stop and back it up first.\n5. Open System Settings → Colors & Themes → Icons or Cursors, choose the theme, and reopen the page if it was already open.\nIf the folder does not contain index.theme, it may be an add-on pack. Download the main theme archive instead.",
                                "Ручная установка:\n1. В файловом менеджере распакуйте скачанный архив.\n2. Откройте распакованные папки и найдите папку темы, в которой лежит index.theme. Нужна именно эта папка, а не внешняя папка архива.\n3. Скопируйте команду ниже. Замените только /path/to/ThemeFolder полным путём к папке темы. Кавычки оставьте — они нужны, если в пути есть пробелы. Пример: \"/home/alex/Downloads/My Theme\".\n4. Откройте Konsole, вставьте изменённую команду и нажмите Enter. Не добавляйте sudo. Если в папке назначения уже есть тема с таким же именем, сначала остановитесь и сделайте её резервную копию.\n5. Откройте «Параметры системы» → «Цвета и темы» → «Значки» или «Курсоры», выберите тему и заново откройте страницу, если она уже была открыта.\nЕсли в папке нет index.theme, это может быть дополнение. Скачайте основной архив темы."
                            )
                            showCopyButton: false
                            maximumHeight: 190
                        }

                        CopyableText {
                            Layout.fillWidth: true
                            visible: root.themeInstallFailed
                            text: {
                                const installDirectory = root.selectedThemeType === 0
                                    ? "$HOME/.local/share/icons"
                                    : "$HOME/.icons"
                                return 'mkdir -p "' + installDirectory
                                    + '" && cp -a "/path/to/ThemeFolder" "' + installDirectory + '/"'
                            }
                            copyButtonText: root.text("Copy command", "Копировать команду")
                            fontFamily: "monospace"
                            maximumHeight: 70
                        }
                    }
                }

                QQC2.ScrollView {
                    id: updateScrollView
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: mainTabs.currentIndex === 4
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: updateScrollView.availableWidth
                        spacing: 10

                        QQC2.Button {
                            Layout.fillWidth: true
                            text: root.busy && root.activeAction === "check-updates"
                                ? root.text("Checking for updates…", "Проверка обновлений…")
                                : root.text("Check for updates", "Проверить обновления")
                            enabled: !root.busy
                            onClicked: root.startUpdateCheck(true)
                        }

                        QQC2.Button {
                            Layout.fillWidth: true
                            visible: root.updateAvailable
                            text: root.text("Update widget", "Обновить виджет")
                            onClicked: Qt.openUrlExternally("https://github.com/neverloseagain1/PlasmaColorPicker")
                        }

                        QQC2.Label {
                            Layout.fillWidth: true
                            text: root.updateStatus
                            wrapMode: Text.Wrap
                            color: root.updateStatusIsError ? "#c01c28" : systemPalette.windowText
                        }

                    }
                }
            }
        }
    }

    FileDialog {
        id: archiveDialog
        title: root.text("Select an icon or cursor theme archive", "Выберите архив темы значков или курсоров")
        nameFilters: [
            root.text(
                "Theme archives (*.zip *.7z *.rar *.tar *.tar.gz *.tgz *.tar.xz *.txz *.tar.bz *.tar.bz2 *.tbz *.tbz2 *.tar.lz *.tar.lzma *.tar.lz4 *.tar.zst *.tzst *.tar.br)",
                "Архивы тем (*.zip *.7z *.rar *.tar *.tar.gz *.tgz *.tar.xz *.txz *.tar.bz *.tar.bz2 *.tbz *.tbz2 *.tar.lz *.tar.lzma *.tar.lz4 *.tar.zst *.tzst *.tar.br)"
            ),
            root.text("All files (*)", "Все файлы (*)")
        ]
        onAccepted: root.installTheme(selectedFile.toString())
    }

    Plasma5Support.DataSource {
        id: commandSource
        engine: "executable"

        onNewData: function(sourceName, data) {
            if (sourceName !== root.activeCommand) {
                disconnectSource(sourceName)
                return
            }
            disconnectSource(sourceName)
            searchTimer.stop()
            root.activeSearch = false
            root.searchAction = ""
            root.searchElapsedSeconds = 0
            root.timedOutSearchAction = ""
            root.activeCommand = ""
            const action = root.activeAction
            root.activeAction = ""
            root.busy = false

            const exitCode = Number(data["exit code"])
            if (exitCode !== 0) {
                let errorText = String(data.stderr || "").trim()
                if (action === "analyze") {
                    errorText = root.analysisErrorMessage(errorText)
                }
                if (action === "check-updates") {
                    root.updateStatus = root.text(
                        "Could not check for updates: %1",
                        "Не удалось проверить обновления: %1"
                    ).replace("%1", errorText.length > 0
                        ? errorText
                        : root.text("the helper exited with code %1.", "помощник завершился с кодом %1.").replace("%1", exitCode))
                    root.updateStatusIsError = true
                    return
                }
                root.statusText = errorText.length > 0
                    ? root.text("Could not complete the request. Details: %1", "Не удалось выполнить действие. Подробности: %1").replace("%1", errorText)
                    : root.text("The helper failed with exit code %1.", "Помощник завершился с кодом %1.").replace("%1", exitCode)
                root.statusIsError = true
                if (action === "icons") {
                    root.storeStatus = root.statusText
                    root.storeStatusIsError = true
                } else if (action === "cursors") {
                    root.cursorStatus = root.statusText
                    root.cursorStatusIsError = true
                } else if (action === "install-theme") {
                    root.showThemeInstallError(errorText.length > 0
                        ? errorText
                        : root.text("the helper exited with code %1.", "помощник завершился с кодом %1.").replace("%1", exitCode))
                }
                return
            }

            try {
                const result = JSON.parse(String(data.stdout || "{}"))
                if (result.error) {
                    const error = action === "analyze"
                        ? root.analysisErrorMessage(result.error)
                        : String(result.error)
                    root.statusText = root.text("Error: %1", "Ошибка: %1").replace("%1", error)
                    root.statusIsError = true
                    if (action === "icons") {
                        root.storeStatus = root.statusText
                        root.storeStatusIsError = true
                    } else if (action === "cursors") {
                        root.cursorStatus = root.statusText
                        root.cursorStatusIsError = true
                    } else if (action === "install-theme") {
                        root.showThemeInstallError(result.error)
                    }
                    return
                }
                if (result.action === "analyze") {
                    root.palettes = result.palettes
                    root.paletteAnalysisType = result.source_type
                    root.diagnosticText = ""
                    root.statusText = result.sample_count > 1
                        ? root.text(
                            "Sampled %1 frames or images from the wallpaper. Choose a palette to save and apply it.",
                            "Для анализа использовано кадров или изображений: %1. Выберите палитру, чтобы сохранить и применить её."
                        )
                            .replace("%1", result.sample_count)
                        : root.text("Choose a palette to save and apply it.", "Выберите палитру, чтобы сохранить и применить её.")
                    root.statusIsError = false
                } else if (result.action === "diagnose") {
                    root.diagnosticText = root.diagnosticResult(result.checks)
                    root.statusText = result.ok
                        ? root.text("No problems found.", "Проблем не обнаружено.")
                        : root.text("Some checks failed. See the diagnostic results below.", "Некоторые проверки не пройдены. Подробности ниже.")
                    root.statusIsError = !result.ok
                } else if (result.action === "apply") {
                    root.statusText = root.text("Color scheme applied and saved.", "Цветовая схема применена и сохранена.")
                    root.statusIsError = false
                    root.diagnosticText = ""
                } else if (result.action === "check-updates") {
                    if (!/^[0-9a-f]{40}$/i.test(result.latest_commit)
                            || !Array.isArray(result.files)
                            || !result.files.every(file => typeof file.path === "string"
                                && ["added", "changed", "removed"].indexOf(file.status) >= 0)) {
                        throw new Error("invalid update check result")
                    }
                    root.updateAvailable = result.files.length > 0
                    root.updateStatus = root.updateAvailable
                        ? root.text(
                            "An update is available. Select Update widget to open the project page.",
                            "Доступно обновление. Нажмите «Обновить виджет», чтобы открыть страницу проекта."
                        )
                        : root.text(
                            "No updates are available.",
                            "Обновлений нет."
                        )
                    root.updateStatusIsError = false
                } else if (result.action === "icons") {
                    root.iconThemes = result.themes
                    root.iconStoreSearchUrl = result.store_search_url || ""
                    root.storeStatusIsError = Boolean(result.catalog_error)
                    root.storeStatus = result.catalog_error
                        ? root.text(
                            "KDE Store's catalog service is unavailable: %1. You can still search the store directly.",
                            "Сервис каталога KDE Store недоступен: %1. Можно открыть поиск непосредственно на сайте."
                        ).replace("%1", result.catalog_error)
                        : result.themes.length > 0
                        ? root.text(
                            "Found %1 icon themes matching %2. Opening an item takes you to KDE Store.",
                            "Найдено тем значков: %1 (цвет %2). Открытие перенаправит на KDE Store."
                        ).replace("%1", result.themes.length).replace("%2", result.search_color)
                        : root.text(
                            "No icon themes matched %1. Try another palette.",
                            "По цвету %1 темы значков не найдены. Попробуйте другую палитру."
                        ).replace("%1", result.search_color)
                } else if (result.action === "cursors") {
                    root.cursorThemes = result.themes
                    root.cursorStoreSearchUrl = result.store_search_url || ""
                    root.cursorStatusIsError = Boolean(result.catalog_error)
                    root.cursorStatus = result.catalog_error
                        ? root.text(
                            "KDE Store's catalog service is unavailable: %1. You can still search the store directly.",
                            "Сервис каталога KDE Store недоступен: %1. Можно открыть поиск непосредственно на сайте."
                        ).replace("%1", result.catalog_error)
                        : result.themes.length > 0
                        ? root.text(
                            "Found %1 cursor themes matching %2. Opening an item takes you to KDE Store.",
                            "Найдено тем курсоров: %1 (цвет %2). Открытие перенаправит на KDE Store."
                        ).replace("%1", result.themes.length).replace("%2", result.search_color)
                        : root.text(
                            "No cursor themes matched %1. Try another palette.",
                            "По цвету %1 темы курсоров не найдены. Попробуйте другую палитру."
                        ).replace("%1", result.search_color)
                } else if (result.action === "install-theme") {
                    if (!Array.isArray(result.themes) || result.themes.length === 0) {
                        root.showThemeInstallError(root.text(
                            "The helper reported success but returned no installed themes.",
                            "Помощник сообщил об успехе, но не вернул список установленных тем."
                        ))
                    } else {
                        const themeNames = result.themes.map(theme => theme.name).join(", ")
                        const installPath = String(result.themes[0].install_path || "")
                        const installDirectory = installPath.substring(0, installPath.lastIndexOf("/"))
                        root.themeInstallFailed = false
                        root.statusIsError = false
                        root.themeInstallStatus = root.text(
                            "Installed %1 theme(s): %2 in %3. Select it in System Settings → Colors & Themes → %4. Reopen that page if it was already open.",
                            "Установлено тем: %1 (%2) в %3. Выберите тему в «Параметры системы» → «Цвета и темы» → «%4». Если страница уже была открыта, откройте её заново."
                        )
                            .replace("%1", result.themes.length)
                            .replace("%2", themeNames)
                            .replace("%3", installDirectory)
                            .replace("%4", result.theme_type === "icons"
                                ? root.text("Icons", "Значки")
                                : root.text("Cursors", "Курсоры"))
                    }
                } else if (result.action === "remove") {
                    root.palettes = []
                    root.iconThemes = []
                    root.cursorThemes = []
                    root.diagnosticText = ""
                    root.statusIsError = false
                    if (result.removed_schemes > 0 || result.removed_theme) {
                        root.statusText = root.text(
                            "Removed %1 widget color schemes and restored the previous appearance. If System Settings was already open, reopen Colors to refresh its list.",
                            "Удалено схем виджета: %1. Предыдущее оформление восстановлено. Если «Параметры системы» были открыты, заново откройте страницу «Цвета», чтобы обновить список."
                        ).replace("%1", result.removed_schemes)
                    } else {
                        root.statusText = root.text(
                            "No saved widget palettes were found. If System Settings was already open, reopen Colors to refresh its list.",
                            "Сохранённых палитр виджета не найдено. Если «Параметры системы» были открыты, заново откройте страницу «Цвета», чтобы обновить список."
                        )
                    }
                    root.storeStatus = root.statusText
                    root.storeStatusIsError = false
                    root.cursorStatus = root.statusText
                    root.cursorStatusIsError = false
                }
            } catch (error) {
                root.statusText = root.text("The helper returned invalid data: %1", "Помощник вернул некорректные данные: %1").replace("%1", error)
                root.statusIsError = true
                if (action === "icons") {
                    root.storeStatus = root.statusText
                    root.storeStatusIsError = true
                } else if (action === "cursors") {
                    root.cursorStatus = root.statusText
                    root.cursorStatusIsError = true
                } else if (action === "install-theme") {
                    root.showThemeInstallError(root.text(
                        "The helper returned invalid data: %1",
                        "Помощник вернул некорректные данные: %1"
                    ).replace("%1", error))
                } else if (action === "check-updates") {
                    root.updateStatus = root.text(
                        "GitHub returned an invalid update response: %1",
                        "GitHub вернул некорректный ответ о проверке обновлений: %1"
                    ).replace("%1", error)
                    root.updateStatusIsError = true
                }
            }
        }
    }

    Timer {
        id: searchTimer
        interval: 1000
        repeat: true
        onTriggered: {
            root.searchElapsedSeconds += 1
            if (root.searchElapsedSeconds >= 45) {
                root.stopSearch(true)
            }
        }
    }

    Timer {
        id: updateCheckTimer
        interval: 500
        repeat: true
        onTriggered: {
            if (!root.expanded || root.updateCheckAttempted) {
                stop()
            } else if (!root.busy) {
                root.startUpdateCheck()
            }
        }
    }
}

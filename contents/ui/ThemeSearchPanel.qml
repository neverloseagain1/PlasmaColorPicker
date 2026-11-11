import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

ColumnLayout {
    id: root

    property bool russian: true
    property bool iconThemes: true
    property bool hasPalettes: false
    property var themes: []
    property string statusText: ""
    property bool busy: false
    property bool searching: false
    property int searchElapsedSeconds: 0
    property bool timedOut: false

    signal searchRequested(int paletteIndex)
    signal openStoreItem(string url)

    SystemPalette {
        id: systemPalette
    }

    spacing: 10

    function text(english, russianText) {
        return root.russian ? russianText : english
    }

    function paletteName(index) {
        const number = String(index + 1).padStart(2, "0")
        return root.text("PlasmaColorPicker " + number, "Палитра PCP " + number)
    }

    QQC2.Label {
        Layout.fillWidth: true
        text: root.iconThemes
            ? root.text(
                "Icon themes matching the selected palette's main color. Results are from KDE Store.",
                "Темы значков по основному цвету выбранной палитры. Результаты предоставляет KDE Store."
            )
            : root.text(
                "Cursor themes matching the selected palette's main color. Results are from KDE Store.",
                "Темы курсоров по основному цвету выбранной палитры. Результаты предоставляет KDE Store."
            )
        wrapMode: Text.Wrap
    }

    RowLayout {
        Layout.fillWidth: true

        QQC2.ComboBox {
            id: paletteSelector
            Layout.fillWidth: true
            model: [
                root.paletteName(0),
                root.paletteName(1),
                root.paletteName(2)
            ]
            displayText: root.hasPalettes
                ? root.paletteName(currentIndex)
                : root.text("Palette " + (currentIndex + 1), "Палитра " + (currentIndex + 1))
        }

        QQC2.Button {
            text: root.busy
                ? root.text("Loading…", "Загрузка…")
                : root.iconThemes
                    ? root.text("Find icons", "Найти значки")
                    : root.text("Find cursors", "Найти курсоры")
            enabled: !root.busy
            onClicked: root.searchRequested(paletteSelector.currentIndex)
        }
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: root.statusText.length > 0
        text: root.statusText
        wrapMode: Text.Wrap
    }

    QQC2.Button {
        visible: root.searching && root.searchElapsedSeconds >= 15
        text: root.text("Cancel search", "Отменить поиск")
        onClicked: root.searchRequested(-1)
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: root.timedOut
        textFormat: Text.RichText
        text: root.text(
            "The search did not return results within the time limit. Please continue your search in <a href=\"https://store.kde.org/\" style=\"color:#268bd2\">KDE Store</a>.",
            "Не удалось получить результаты поиска за отведённое время. Продолжите поиск самостоятельно в <a href=\"https://store.kde.org/\" style=\"color:#268bd2\">KDE Store</a>."
        )
        wrapMode: Text.Wrap
        onLinkActivated: function(link) {
            Qt.openUrlExternally(link)
        }
    }

    ListView {
        id: themeList
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true
        spacing: 8
        model: root.themes

        delegate: Rectangle {
            required property var modelData
            width: themeList.width
            height: 110
            radius: 8
            color: systemPalette.alternateBase

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 10
                spacing: 4

                QQC2.Label {
                    Layout.fillWidth: true
                    text: modelData.name
                    font.bold: true
                    elide: Text.ElideRight
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    text: root.text("By ", "Автор: ") + modelData.author
                    font.pointSize: 9
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    text: modelData.summary
                    wrapMode: Text.Wrap
                    elide: Text.ElideRight
                    maximumLineCount: 2
                    font.pointSize: 9
                }

                QQC2.Button {
                    Layout.alignment: Qt.AlignRight
                    text: root.text("View in KDE Store", "Открыть в KDE Store")
                    onClicked: root.openStoreItem(modelData.detail_url)
                }
            }
        }
    }
}

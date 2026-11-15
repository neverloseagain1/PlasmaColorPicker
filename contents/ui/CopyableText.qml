// Read-only, scrollable text area with an optional copy button.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

Item {
    id: root

    property string text: ""
    property string copyButtonText: "Copy"
    property string fontFamily: ""
    property int maximumHeight: 140
    property bool showCopyButton: true

    implicitHeight: contentColumn.implicitHeight

    SystemPalette {
        id: systemPalette
    }

    ColumnLayout {
        id: contentColumn
        anchors.fill: parent
        spacing: 2

        QQC2.ScrollView {
            id: textScrollView
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(Math.max(contentText.implicitHeight, 32), root.maximumHeight)
            contentWidth: availableWidth

            QQC2.TextArea {
                id: contentText
                width: textScrollView.availableWidth
                text: root.text
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
                font.family: root.fontFamily.length > 0 ? root.fontFamily : "sans-serif"
                color: systemPalette.windowText
                padding: 6
                background: Rectangle {
                    color: systemPalette.base
                    radius: 4
                    border.color: systemPalette.mid
                }
            }
        }

        QQC2.Button {
            Layout.alignment: Qt.AlignRight
            visible: root.showCopyButton
            text: root.copyButtonText
            icon.name: "edit-copy"
            onClicked: {
                contentText.selectAll()
                contentText.copy()
                contentText.deselect()
            }
        }
    }
}

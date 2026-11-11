import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.plasma.plasmoid

Kirigami.FormLayout {
    id: root

    property string cfg_language: Plasmoid.configuration.language
    property bool cfg_showDiagnostics: Plasmoid.configuration.showDiagnostics

    function text(english, russian) {
        return root.cfg_language === "en" ? english : russian
    }

    QQC2.ComboBox {
        id: languageSelector
        Kirigami.FormData.label: root.text("Language:", "Язык:")
        textRole: "label"
        valueRole: "value"
        model: [
            { "label": root.text("Russian", "Русский"), "value": "ru" },
            { "label": root.text("English", "Английский"), "value": "en" }
        ]
        currentIndex: indexOfValue(root.cfg_language)
        onActivated: root.cfg_language = currentValue
    }

    QQC2.CheckBox {
        id: diagnosticsToggle
        Kirigami.FormData.label: root.text("Diagnostics:", "Диагностика:")
        text: root.text(
            "Show the error-check button in the widget",
            "Показывать кнопку проверки в виджете"
        )
        checked: root.cfg_showDiagnostics
        onToggled: root.cfg_showDiagnostics = checked
    }
}

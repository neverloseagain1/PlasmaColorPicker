// Registers the General settings page in Plasma's configuration dialog.
import QtQuick
import org.kde.plasma.configuration

ConfigModel {
    ConfigCategory {
        name: i18n("General")
        icon: "preferences-system"
        source: "config/GeneralConfig.qml"
    }
}

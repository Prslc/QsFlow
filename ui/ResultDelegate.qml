import QtQuick
import QtQuick.Controls

ItemDelegate {
    id: root
    width: ListView.view.width
    hoverEnabled: true

    // simple mode
    readonly property bool isSimpleMode: (model.summary === undefined || model.summary === "")
                                      && (model.icon === undefined || model.icon === "")

    // row height — SearchWindow.rowHeight() mirrors this; keep in sync
    implicitHeight: isSimpleMode ? 48 : 64
    highlighted: ListView.isCurrentItem
    leftPadding: 12
    rightPadding: 12
    topPadding: isSimpleMode ? 4 : 6
    bottomPadding: isSimpleMode ? 4 : 6

    background: Rectangle {
        radius: 8
        anchors.margins: 2
        color: root.highlighted ? backend.theme.primary
             : root.hovered   ? Qt.alpha(backend.theme.primary, 0.15)
             : "transparent"

        Behavior on color {
            ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
        }
    }

    contentItem: Row {
        id: contentLayout
        spacing: 12
        anchors.fill: parent
        anchors.margins: 8

        // icon
        Image {
            id: iconSource
            asynchronous: true
            mipmap: true
            source: (model.icon && model.icon !== "") ? "file://" + model.icon : ""
            width: root.isSimpleMode ? 24 : 32
            height: root.isSimpleMode ? 24 : 32
            anchors.verticalCenter: parent.verticalCenter
            visible: model.icon !== undefined && model.icon !== "" && source !== "" && status !== Image.Error
            fillMode: Image.PreserveAspectFit
            // fade icons in instead of popping; cached loads are near-instant
            opacity: (source === "" || status === Image.Ready) ? 1 : 0
            Behavior on opacity {
                NumberAnimation { duration: 100; easing.type: Easing.OutCubic }
            }
        }

        Column {
            width: parent.width - (iconSource.visible ? iconSource.width + parent.spacing : 0)
            spacing: 2
            anchors.verticalCenter: parent.verticalCenter

            // title
            Text {
                text: model.title
                color: root.highlighted ? backend.theme.on_primary : backend.theme.fg
                font.bold: true
                font.pixelSize: root.isSimpleMode ? 16 : 14

                elide: Text.ElideRight
                width: parent.width

                Behavior on color {
                    ColorAnimation { duration: 150; easing.type: Easing.OutCubic }
                }
            }
            // summary
            Text {
                text: model.summary || ""
                color: root.highlighted ? backend.theme.on_primary : backend.theme.fg
                font.pixelSize: 12
                elide: Text.ElideRight
                width: parent.width
                visible: text !== ""
                opacity: 0.8

                Behavior on color {
                    ColorAnimation { duration: 150; easing.type: Easing.OutCubic }
                }
            }
        }
    }
}

import QtQuick
import QtQuick.Controls
import Quickshell.Widgets

ItemDelegate {
    id: root
    width: ListView.view.width
    hoverEnabled: true

    // simple mode — no icon, no summary
    readonly property bool isSimpleMode: (model.summary === undefined || model.summary === "")
                                      && (model.icon === undefined || model.icon === "")

    // row height — SearchWindow.rowHeight() mirrors this; keep in sync
    implicitHeight: isSimpleMode ? 48 : 64
    highlighted: ListView.isCurrentItem
    leftPadding: 0
    rightPadding: 0
    topPadding: 0
    bottomPadding: 0

    background: Rectangle {
        radius: 8
        anchors.margins: 1
        color: root.highlighted ? Qt.alpha(backend.theme.primary, 0.15)
             : root.hovered   ? Qt.alpha(backend.theme.primary, 0.08)
             : "transparent"

        Behavior on color {
            ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
        }

        // accent bar marking the selected row
        Rectangle {
            anchors.left: parent.left
            anchors.leftMargin: 3
            anchors.verticalCenter: parent.verticalCenter
            width: 3
            height: parent.height * 0.45
            radius: 1.5
            color: backend.theme.primary
            visible: root.highlighted
        }
    }

    contentItem: Row {
        id: contentLayout
        spacing: 12
        anchors.fill: parent
        anchors.margins: 10

        // icon
        IconImage {
            id: iconSource
            source: (model.icon && model.icon !== "") ? "file://" + model.icon : ""
            implicitSize: root.isSimpleMode ? 22 : 30
            asynchronous: true
            anchors.verticalCenter: parent.verticalCenter
            visible: model.icon !== undefined && model.icon !== "" && status !== Image.Error
            // fade icons in instead of popping; cached loads are near-instant
            opacity: (source === "" || status === Image.Ready) ? 1 : 0
            Behavior on opacity {
                NumberAnimation { duration: 100; easing.type: Easing.OutCubic }
            }
        }

        Column {
            width: parent.width
                 - (iconSource.visible ? iconSource.implicitSize + parent.spacing : 0)
                 - (enterHint.visible ? enterHint.implicitWidth + parent.spacing : 0)
            spacing: 2
            anchors.verticalCenter: parent.verticalCenter

            // title
            Text {
                text: model.title
                color: backend.theme.fg
                font.bold: true
                font.pixelSize: root.isSimpleMode ? 15 : 14
                elide: Text.ElideRight
                width: parent.width

                Behavior on color {
                    ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
            }

            // summary
            Text {
                text: model.summary || ""
                color: backend.theme.fg
                font.pixelSize: 12
                elide: Text.ElideRight
                width: parent.width
                visible: text !== ""
                opacity: 0.7

                Behavior on color {
                    ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
            }
        }

        // enter hint on the selected row
        Text {
            id: enterHint
            text: "↵"
            font.pixelSize: 13
            color: Qt.alpha(backend.theme.primary, 0.55)
            anchors.verticalCenter: parent.verticalCenter
            visible: root.highlighted
        }
    }
}

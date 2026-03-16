import QtQuick
import QtQuick.Controls

ItemDelegate {
    id: root
    width: ListView.view.width

    // simple mode
    readonly property bool isSimpleMode: (model.summary === undefined || model.summary === "")
                                      && (model.icon === undefined || model.icon === "")

    implicitHeight: isSimpleMode ? 48 : 64
    highlighted: ListView.isCurrentItem
    leftPadding: 12
    rightPadding: 12
    topPadding: isSimpleMode ? 4 : 6
    bottomPadding: isSimpleMode ? 4 : 6

    background: Rectangle {
        radius: 8
        anchors.margins: 2
        color: root.highlighted ? backend.theme.primary : "transparent"
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
            source: (model.icon && model.icon !== "") ? "file://" + model.icon : ""
            width: root.isSimpleMode ? 24 : 32
            height: root.isSimpleMode ? 24 : 32
            anchors.verticalCenter: parent.verticalCenter
            visible: model.icon !== undefined && model.icon !== ""
            fillMode: Image.PreserveAspectFit
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

                Behavior on font.pixelSize {
                    NumberAnimation { duration: 100 }
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
            }
        }
    }
}
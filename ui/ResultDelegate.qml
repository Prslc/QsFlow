import QtQuick
import QtQuick.Controls

ItemDelegate {
    id: root
    width: ListView.view.width
    
    implicitHeight: contentLayout.implicitHeight + 16 

    highlighted: ListView.isCurrentItem

    // simple mode
    readonly property bool isSimpleMode: (model.summary === undefined || model.summary === "")
                                      && (model.icon === undefined || model.icon === "")

    background: Rectangle {
        radius: 8
        color: root.highlighted ? "#24283b" : "transparent"
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
            width: 32
            height: 32
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
                color: "#c0caf5"
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
                color: "#565f89"
                font.pixelSize: 12
                elide: Text.ElideRight
                width: parent.width
                visible: text !== ""
            }
        }
    }
}
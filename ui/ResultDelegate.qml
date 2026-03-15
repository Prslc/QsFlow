import QtQuick
import QtQuick.Controls

ItemDelegate {
    id: root
    width: ListView.view.width
    
    implicitHeight: contentLayout.implicitHeight + 16 

    highlighted: ListView.isCurrentItem

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
            source: model.icon ? model.icon : ""
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
                font.pixelSize: 14
                elide: Text.ElideRight
                width: parent.width
            }

            // Summary
            Text {
                text: model.summary ? model.summary : ""
                color: "#565f89"
                font.pixelSize: 12
                elide: Text.ElideRight
                width: parent.width
                visible: model.summary !== undefined && model.summary !== ""
            }
        }
    }
}
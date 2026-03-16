import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: window

    Component.onCompleted: {
        if (this.WlrLayershell !== undefined) {
            this.WlrLayershell.layer = WlrLayer.Overlay;    // Overlay display
            this.WlrLayershell.namespace = "QsFlow";
        }
    }

    MouseArea {
        anchors.fill: parent
        onClicked: Qt.quit()
    }

    property ListModel resultsModel
    property var theme: backend.theme

    signal searchTriggered(string text)

    anchors { top: true; left: true; right: true }
    margins { top: 100 }

    implicitHeight: 500
    exclusiveZone: 0
    aboveWindows: true
    focusable: true
    color: "transparent"

    Rectangle {
        id: content
        anchors.horizontalCenter: parent.horizontalCenter
        width: 600
        height: (searchInput.text.length === 0 || resultsModel.count === 0)
                ? 72
                : Math.min(childrenRect.height + 25, 470)

        Behavior on height {
            NumberAnimation { duration: 150; easing.type: Easing.OutCubic }
        }

        color: backend.theme.container
        radius: 12
        border.color: backend.theme.primary
        border.width: 1
        clip: true

        Column {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 12
            spacing: 10

            TextField {
                id: searchInput
                height: 48
                width: parent.width
                color: backend.theme.fg
                font.pixelSize: 20
                leftPadding: 12
                verticalAlignment: TextInput.AlignVCenter
                placeholderText: "QsFlow: Search..."
                placeholderTextColor: Qt.alpha(backend.theme.fg, 0.5)
                focus: true
                background: null

                Keys.onEscapePressed: Qt.quit()

                onTextChanged: window.searchTriggered(text)

                Keys.onDownPressed: {
                    resultsList.currentIndex = Math.min(
                        resultsList.currentIndex + 1,
                        resultsModel.count - 1
                    )
                }

                Keys.onUpPressed: {
                    resultsList.currentIndex = Math.max(
                        resultsList.currentIndex - 1,
                        0
                    )
                }

                Keys.onReturnPressed: {
                    let item = resultsModel.get(resultsList.currentIndex)
                    if (item && item.on_click) {
                        window.launch(item.on_click)
                    }
                }
            }

            Rectangle {
                width: parent.width
                height: 1
                color: backend.theme.primary
                opacity: 0.2
                visible: resultsModel.count > 0
            }

            ListView {
                id: resultsList
                width: parent.width
                implicitHeight: Math.min(resultsModel.count * 64, 320)
                model: resultsModel
                clip: true
                highlightMoveDuration: 0
                onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
                delegate: ResultDelegate {
                    onClicked: window.launch(model.on_click)
                }
            }
        }
    }
    function launch(target) {
        if (!target) return

        let isUrl = target.startsWith("http") ||
                    target.startsWith("file:") ||
                    target.startsWith("mailto:")

        if (target.startsWith("run:")) {
            window.searchTriggered("run " + target.substring(4))
            exitTimer.start()
        } else if (isUrl) {
            Qt.openUrlExternally(target)
            Qt.quit()
        } else {
            window.searchTriggered("run " + target)
            exitTimer.start()
        }
    }

    // wait exec over
    Timer {
        id: exitTimer
        interval: 150
        onTriggered: Qt.quit()
    }
}
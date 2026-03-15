import QtQuick
import QtQuick.Controls
import Quickshell

PanelWindow {
    id: window
    property ListModel resultsModel
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
        height: searchInput.text.length > 0 ? 450 : 70

        Behavior on height {
            NumberAnimation { duration: 150; easing.type: Easing.OutCubic }
        }

        color: "#1a1b26"
        radius: 12
        border.color: "#7aa2f7"
        border.width: 1
        clip: true

        Column {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 10

            TextField {
                id: searchInput
                height: 48
                width: parent.width
                color: "#c0caf5"
                font.pixelSize: 20
                placeholderText: "QsFlow: Search..."
                placeholderTextColor: "#565f89"
                focus: true
                background: null

                Keys.onEscapePressed: Qt.quit()
                onActiveFocusChanged: if (!activeFocus) Qt.quit()

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
                color: "#414868"
                visible: resultsModel.count > 0
            }

            ListView {
                id: resultsList
                width: parent.width
                height: 360
                model: resultsModel
                clip: true
                currentIndex: 0
                delegate: ResultDelegate {
                    onClicked: {
                        window.launch(model.on_click)
                    }
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
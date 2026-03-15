import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io

ShellRoot {

    ListModel {
        id: resultsModel
    }

    Process {
        id: backend
        command: ["core/target/debug/core"]
        running: true
        stdinEnabled: true

        stdout: SplitParser {
            splitMarker: "\n"
            onRead: (line) => {
                line = line.trim()
                if (line.length === 0) return

                try {
                    let json = JSON.parse(line)
                    resultsModel.clear()
                    for (let item of json)
                        resultsModel.append(item)
                } catch (e) {
                    console.log("Parse error:", e)
                }
            }
        }
    }

    PanelWindow {
        id: window
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

                    onActiveFocusChanged: {
                        if (!activeFocus)
                            Qt.quit()
                    }

                    onTextChanged: {
                        if (text.length === 0) {
                            resultsModel.clear()
                            return
                        }
                        backend.write(text + "\n")
                    }

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
                        if (item && item.on_click)
                            Qt.openUrlExternally(item.on_click)
                        Qt.quit()
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

                    delegate: ItemDelegate {
                        width: resultsList.width
                        implicitHeight: 50

                        highlighted: ListView.isCurrentItem

                        background: Rectangle {
                            radius: 8
                            color: highlighted ? "#24283b" : "transparent"
                        }

                        contentItem: Column {
                            anchors.fill: parent
                            anchors.margins: 8
                            spacing: 2

                            Text {
                                text: model.title
                                color: "#c0caf5"
                                font.bold: true
                                elide: Text.ElideRight
                                width: parent.width
                                visible: model.title && model.title.length > 0
                            }

                            Text {
                                text: model.summary
                                color: "#565f89"
                                font.pixelSize: 12
                                elide: Text.ElideRight
                                width: parent.width
                                visible: model.summary && model.summary.length > 0
                            }
                        }

                        onClicked: {
                            if (model.on_click)
                                Qt.openUrlExternally(model.on_click)
                            Qt.quit()
                        }
                    }
                }
            }
        }
    }
}
import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: window
    // Resident mode: the systemd unit sets QSFLOW_RESIDENT=1 → start hidden and
    // toggle via `ipc call launcher toggle`. A manual `quickshell -p ...` (no
    // env) keeps the old behaviour: show at launch and quit on dismiss.
    readonly property bool resident: Quickshell.env("QSFLOW_RESIDENT") === "1"
    visible: !resident

    Component.onCompleted: {
        if (this.WlrLayershell !== undefined) {
            this.WlrLayershell.layer = WlrLayer.Overlay
            this.WlrLayershell.namespace = "QsFlow"
        }
        window.BackgroundEffect.blurRegion = blurRegion
        entrance.restart()
        initTimer.start()
    }

    // Frosted glass: compositor-side blur (ext-background-effect) exactly over
    // the rounded card, so only the card area is blurred.
    Region {
        id: blurRegion
        item: content
        radius: content.radius
    }

    // initial empty search -> usage-ranked history
    Timer {
        id: initTimer
        interval: 120
        onTriggered: window.searchTriggered("")
    }

    property ListModel resultsModel
    property var theme: backend.theme

    // Keep the selection valid when the result count shrinks; auto-select the
    // first row when nothing is selected yet (ListView starts at -1).
    Connections {
        target: resultsModel
        function onCountChanged() {
            if (resultsList.currentIndex > resultsModel.count - 1)
                resultsList.currentIndex = Math.max(resultsModel.count - 1, 0)
            else if (resultsList.currentIndex < 0 && resultsModel.count > 0)
                resultsList.currentIndex = 0
        }
    }

    // A genuinely new result payload replaced the list → snap back to the
    // first row (typing a new query starts from the top result). Deliberately
    // NOT on every count change: local removals (forget) shift the selection
    // in place, and identical re-sends never reach this signal, so those two
    // flows keep their cursor position.
    Connections {
        target: backend
        function onResultsUpdated() {
            if (resultsModel.count > 0)
                resultsList.currentIndex = 0
        }
    }

    signal searchTriggered(string text)

    // full-screen overlay
    anchors { top: true; left: true; right: true; bottom: true }
    exclusiveZone: 0
    aboveWindows: true
    focusable: true
    color: "transparent"

    readonly property int rowH: 64
    readonly property int maxRows: 5

    // Exact list height: every row is rowH tall (delegates use window.rowH),
    // capped at maxRows rows.
    function listHeight() {
        return Math.min(resultsModel.count * rowH, maxRows * rowH)
    }

    // card geometry
    readonly property int cardPad: 14
    readonly property int searchH: 52
    readonly property int footerH: 28
    readonly property int gap: 10
    function contentHeight() {
        const hasResults = resultsModel.count > 0
        let h = cardPad + searchH + gap + footerH + cardPad
        if (hasResults)
            h += gap + listHeight()
        return Math.min(h, 480)
    }

    // dimmed backdrop — click outside the card dismisses
    Rectangle {
        id: dim
        anchors.fill: parent
        color: "#000000"
        opacity: 0
    }
    MouseArea {
        anchors.fill: parent
        onClicked: window.close()
    }

    // centered card
    Item {
        anchors.fill: parent

        Rectangle {
            id: content
            anchors.centerIn: parent
            width: Math.min(Math.max(560, parent.width * 0.38), 760)
            height: contentHeight()

            Behavior on height {
                NumberAnimation { duration: 150; easing.type: Easing.OutCubic }
            }

            radius: 16
            color: Qt.alpha(backend.theme.container, 0.75)
            clip: true
            opacity: 0
            scale: 0.97

            // swallow clicks on card padding — only outside clicks dismiss
            MouseArea {
                anchors.fill: parent
                onClicked: (mouse) => mouse.accepted = true
            }

            Column {
                anchors.fill: parent
                anchors.margins: cardPad
                spacing: gap

                SearchBar {
                    id: searchBar
                    width: parent.width
                    fg: backend.theme.fg
                    accent: backend.theme.primary
                    dim: Qt.alpha(backend.theme.fg, 0.55)
                    onTextChanged: window.searchTriggered(text)
                    onLaunchRequested: {
                        if (inputField.inputMethodComposing && searchBar.text === "") return
                        window.launchCurrent()
                    }
                    onDismissRequested: window.close()
                    onMoveUp: resultsList.currentIndex = Math.max(resultsList.currentIndex - 1, 0)
                    onMoveDown: resultsList.currentIndex = Math.min(resultsList.currentIndex + 1, resultsModel.count - 1)
                    onPageUp: resultsList.currentIndex = Math.max(resultsList.currentIndex - 5, 0)
                    onPageDown: resultsList.currentIndex = Math.min(resultsList.currentIndex + 5, resultsModel.count - 1)
                    onGoHome: resultsList.currentIndex = 0
                    onGoEnd: resultsList.currentIndex = Math.max(resultsModel.count - 1, 0)
                    onForgetRequested: window.forgetCurrent()
                }

                ListView {
                    id: resultsList
                    width: parent.width
                    height: listHeight()
                    visible: resultsModel.count > 0
                    model: resultsModel
                    clip: true
                    highlightMoveDuration: 0
                    onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
                    delegate: ResultDelegate {
                        onClicked: {
                            ListView.view.currentIndex = index
                            window.launch(model.on_click)
                        }
                    }
                }

                // footer: key hints + result count
                Item {
                    width: parent.width
                    height: footerH

                    Text {
                        anchors.left: parent.left
                        anchors.verticalCenter: parent.verticalCenter
                        font.pixelSize: 11
                        color: Qt.alpha(backend.theme.fg, 0.5)
                        text: resultsModel.count === 0
                            ? "Type ? for help  ·  prefixes: b h f d c s g tr r w"
                            : "↵ Launch   ↑↓ Move   ⌫ Forget   Esc Close"
                    }

                    Text {
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        font.pixelSize: 11
                        color: Qt.alpha(backend.theme.fg, 0.45)
                        text: resultsModel.count === 0 ? "" : resultsModel.count + " results"
                    }
                }
            }
        }
    }

    ParallelAnimation {
        id: entrance
        NumberAnimation {
            target: dim
            property: "opacity"
            to: 0.30
            duration: 160
            easing.type: Easing.OutCubic
        }
        NumberAnimation {
            target: content
            property: "opacity"
            to: 1
            duration: 200
            easing.type: Easing.OutCubic
        }
        NumberAnimation {
            target: content
            property: "scale"
            to: 1
            duration: 200
            easing.type: Easing.OutCubic
        }
    }

    function currentItem() {
        return resultsModel.get(resultsList.currentIndex)
    }

    function launchCurrent() {
        const item = currentItem()
        if (item && item.on_click)
            window.launch(item.on_click)
    }

    function forgetCurrent() {
        const item = currentItem()
        if (!item || !item.on_click) return
        backend.write("forget " + item.on_click + "\n")
        resultsModel.remove(resultsList.currentIndex)
        resultsList.currentIndex = Math.min(resultsList.currentIndex, resultsModel.count - 1)
    }

    function launch(target) {
        if (!target) return

        let idx = resultsList.currentIndex
        let item = resultsModel.get(idx)
        if (item && item.on_click) {
            backend.write("select " + JSON.stringify({
                title: item.title,
                summary: item.summary || "",
                on_click: item.on_click,
                icon: item.icon || ""
            }) + "\n")
        }

        let isUrl = target.startsWith("http") ||
                    target.startsWith("file:") ||
                    target.startsWith("mailto:")

        if (target.startsWith("launch:")) {
            window.searchTriggered("launch " + target.substring(7))
        } else if (target.startsWith("run:")) {
            window.searchTriggered("run " + target.substring(4))
        } else if (target.startsWith("copy:")) {
            window.searchTriggered("copy " + target.substring(5))
        } else if (isUrl) {
            Qt.openUrlExternally(target)
        } else {
            window.searchTriggered("run " + target)
        }
        exitTimer.start()
    }
    function open() {
        visible = true
        searchBar.text = ""
        window.searchTriggered("")
        dim.opacity = 0
        content.opacity = 0
        content.scale = 0.97
        entrance.restart()
        focusTimer.restart()
    }

    function close() {
        if (resident)
            visible = false
        else
            Qt.quit()
    }

    function toggle() {
        if (visible) close(); else open()
    }

    Timer {
        id: focusTimer
        interval: 20
        onTriggered: searchBar.inputField.forceActiveFocus()
    }

    Timer {
        id: exitTimer
        interval: 150
        onTriggered: window.close()
    }
}

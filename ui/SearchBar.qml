import QtQuick
import QtQuick.Controls

// Search input row: magnifier icon, text field, active keyword-prefix chip and a
// clear button. Pure input UX — navigation/launch keys are forwarded as signals
// so the window owns selection and launch logic.
Item {
    id: root

    height: 52
    width: parent ? parent.width : 0

    property alias text: input.text
    property alias inputField: input
    property string placeholderText: "Search apps, files, web..."
    property color fg: "#c0caf5"
    property color accent: "#7aa2f7"
    property color dim: Qt.alpha("#c0caf5", 0.55)

    signal launchRequested()
    signal dismissRequested()

    Component.onCompleted: {
        // focus: true is not enough — the window is not active at creation
        // time, so activeFocus stays false and the focus ring never shows
        input.forceActiveFocus()
    }
    signal moveUp()
    signal moveDown()
    signal pageUp()
    signal pageDown()
    signal goHome()
    signal goEnd()
    signal forgetRequested()

    // Active keyword prefix (b, h, f, ...) derived from input text.
    readonly property string modePrefix: {
        const m = /^([a-zA-Z]{1,3})\s/.exec(input.text)
        return m ? m[1] : ""
    }

    // recessed input field
    Rectangle {
        id: field
        anchors.fill: parent
        radius: 9
        color: Qt.alpha(root.fg, 0.08)
    }

    // magnifier glyph, drawn to avoid asset/font dependencies
    Canvas {
        id: searchIcon
        anchors.left: parent.left
        anchors.leftMargin: 14
        anchors.verticalCenter: parent.verticalCenter
        width: 22
        height: 22
        antialiasing: true
        onPaint: {
            const ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)
            ctx.strokeStyle = root.dim
            ctx.lineWidth = 2
            ctx.beginPath()
            ctx.arc(9, 9, 6.2, 0, Math.PI * 2)
            ctx.moveTo(13.8, 13.8)
            ctx.lineTo(19, 19)
            ctx.stroke()
        }
    }

    Connections {
        target: root
        function onDimChanged() { searchIcon.requestPaint() }
    }

    TextField {
        id: input
        anchors.left: searchIcon.right
        anchors.right: toolbar.left
        anchors.leftMargin: 12
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        height: 36

        color: root.fg
        font.pixelSize: 18
        selectionColor: Qt.alpha(root.accent, 0.35)
        selectedTextColor: root.fg
        placeholderText: root.placeholderText
        placeholderTextColor: root.dim
        background: null
        focus: true

        onAccepted: root.launchRequested()
        Keys.onEscapePressed: (event) => root.dismissRequested()
        Keys.onUpPressed: (event) => root.moveUp()
        Keys.onDownPressed: (event) => root.moveDown()
        Keys.onDeletePressed: (event) => root.forgetRequested()
        Keys.onPressed: (event) => {
            // Home/End/PageUp/PageDown have no dedicated Keys signals in Qt 6.11
            if (event.key === Qt.Key_PageUp) {
                root.pageUp()
                event.accepted = true
            } else if (event.key === Qt.Key_PageDown) {
                root.pageDown()
                event.accepted = true
            } else if (event.key === Qt.Key_Home) {
                root.goHome()
                event.accepted = true
            } else if (event.key === Qt.Key_End) {
                root.goEnd()
                event.accepted = true
            }
        }
    }

    Row {
        id: toolbar
        anchors.right: parent.right
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        spacing: 6

        // keyword prefix chip
        Rectangle {
            id: modeChip
            visible: root.modePrefix !== ""
            implicitWidth: chipLabel.implicitWidth + 16
            implicitHeight: 24
            radius: 6
            color: Qt.alpha(root.accent, 0.16)

            Text {
                id: chipLabel
                anchors.centerIn: parent
                text: root.modePrefix
                font.pixelSize: 11
                font.bold: true
                color: root.accent
            }

            ToolTip {
                text: "Keyword mode — results scoped to this prefix"
                visible: chipHover.containsMouse
                delay: 500
            }

            MouseArea {
                id: chipHover
                anchors.fill: parent
                hoverEnabled: true
            }
        }

        // clear button
        Rectangle {
            id: clearButton
            width: 26
            height: 26
            radius: 13
            color: clearHover.containsMouse ? Qt.alpha(root.fg, 0.12) : "transparent"
            visible: input.text !== ""

            // mouse-hover state feedback fades in (ui-animation: colour state
            // feedback may transition; hover is mouse-driven)
            Behavior on color {
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }

            Text {
                text: "✕"
                anchors.centerIn: parent
                font.pixelSize: 12
                color: root.dim
            }

            MouseArea {
                id: clearHover
                anchors.fill: parent
                hoverEnabled: true
                onClicked: {
                    input.clear()
                    input.forceActiveFocus()
                }
            }
        }
    }
}

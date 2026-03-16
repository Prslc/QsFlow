import QtQuick
import Quickshell.Io

Process {
    id: backend
    property ListModel model

    property var theme: ({ 
        "primary": "#7aa2f7",
        "bg": "#1a1b26",
        "fg": "#c0caf5",
        "container": "#24283b",
        "on_primary": "#1a1b26"
    })

    command: ["bin/qsflow-core"]
    running: true
    stdinEnabled: true

    stdout: SplitParser {
        splitMarker: "\n"
        onRead: (line) => {
            line = line.trim()
            if (line.length === 0) return

            try {
                let msg = JSON.parse(line)

                if (msg.type === "theme") {
                    backend.theme = msg.data
                    console.log("Theme updated from backend")
                }
                else if (msg.type === "results") {
                    if (backend.model) {
                        backend.model.clear()
                        for (let item of msg.data)
                            backend.model.append(item)
                    }
                }
                else if (Array.isArray(msg)) {
                    if (backend.model) {
                        backend.model.clear()
                        for (let item of msg)
                            backend.model.append(item)
                    }
                }
            } catch (e) {
                console.log("JSON Parse error:", e)
            }
        }
    }

    function sendSearch(text) {
        if (text.length === 0) {
            if (model) model.clear()
            return
        }
        write(text + "\n")
    }
}
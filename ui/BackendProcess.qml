import QtQuick
import Quickshell.Io

Process {
    id: backend
    property ListModel model

    command: ["bin/qsflow-core"]
    running: true
    stdinEnabled: true

    stdout: SplitParser {
        splitMarker: "\n"
        onRead: (line) => {
            line = line.trim()
            if (line.length === 0) return

            try {
                let json = JSON.parse(line)
                if (backend.model) {
                    backend.model.clear()
                    for (let item of json)
                        backend.model.append(item)
                }
            } catch (e) {
                console.log("Parse error:", e)
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
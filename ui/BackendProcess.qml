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

    // last emitted result payload — used to skip identical result sets
    property string lastResults: ""

    // emitted whenever a genuinely new result payload replaces the list —
    // the window snaps its selection back to the first row on this
    signal resultsUpdated()

    command: ["qsflow-core"]
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
                    backend.applyResults(msg.data)
                }
                else if (Array.isArray(msg)) {
                    backend.applyResults(msg)
                }
            } catch (e) {
                console.log("JSON Parse error:", e)
            }
        }
    }

    // Identical result sets (e.g. re-typing the same query) are dropped —
    // rebuilding would reset selection. Otherwise patch rows in place:
    // delegates survive result changes, so highlight, hover and icon state
    // never flash; only changed rows re-bind.
    function applyResults(items) {
        if (!model) return
        let json = JSON.stringify(items)
        if (json === lastResults && model.count > 0) return
        lastResults = json

        let oldLen = model.count
        let newLen = items.length
        let common = Math.min(oldLen, newLen)
        for (let i = 0; i < common; i++) {
            if (JSON.stringify(model.get(i)) !== JSON.stringify(items[i]))
                model.set(i, items[i])
        }
        if (newLen < oldLen)
            model.remove(newLen, oldLen - newLen)
        else
            for (let i = oldLen; i < newLen; i++)
                model.append(items[i])
        resultsUpdated()
    }

    function sendSearch(text) {
        write(text + "\n")
    }

    onExited: {
        if (model) model.clear()
    }
}

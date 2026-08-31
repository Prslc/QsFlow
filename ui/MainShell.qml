import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
    ListModel {
        id: resultsModel
    }

    BackendProcess {
        id: backend
        model: resultsModel
    }

    SearchWindow {
        id: window
        resultsModel: resultsModel
        onSearchTriggered: (text) => backend.sendSearch(text)
    }

    // Resident-mode IPC surface: `quickshell ipc ... call launcher <open|close|toggle>`
    // toggles the launcher window without re-spawning the shell (and thus the core).
    IpcHandler {
        target: "launcher"
        function open(): string {
            window.open()
            return "OPEN"
        }
        function close(): string {
            window.close()
            return "CLOSE"
        }
        function toggle(): string {
            window.toggle()
            return "TOGGLE"
        }
    }
}

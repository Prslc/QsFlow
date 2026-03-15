import QtQuick
import Quickshell

ShellRoot {
    ListModel {
        id: resultsModel
    }

    BackendProcess {
        id: backend
        model: resultsModel
    }

    SearchWindow {
        resultsModel: resultsModel
        onSearchTriggered: (text) => backend.sendSearch(text)
    }
}
import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: searchPage

    property var searchResults: []
    property bool loading: false
    property string lastQuery: ""

    // Called from main.qml topbar search — triggers search immediately
    function triggerSearch(q) {
        searchField.text = q;
        lastQuery = q;
        searchResults = [];
        loading = true;
        backend.search(q);
        pollTimer.start();
    }

    Timer {
        id: debounce
        interval: 350
        repeat: false
        onTriggered: {
            var q = searchField.text.trim();
            if (q.length < 2 || q === lastQuery) return;
            lastQuery = q;
            searchResults = [];
            loading = true;
            backend.search(q);
            pollTimer.start();
        }
    }

    Timer {
        id: pollTimer
        interval: 400
        repeat: true
        onTriggered: {
            backend.pollOp();
            if (!backend.opRunning) {
                pollTimer.stop();
                loading = false;
                if (backend.opResult === 1) {
                    try { searchResults = JSON.parse(backend.readLog()); }
                    catch(e) { searchResults = []; }
                }
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Search bar ──────────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: 56
            color: palette.button

            RowLayout {
                anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
                spacing: 10

                Label {
                    text: "🔍"
                    font.pixelSize: 18
                }

                TextField {
                    id: searchField
                    Layout.fillWidth: true
                    placeholderText: "Search apps, packages, web apps…"
                    font.pixelSize: 14
                    background: Rectangle { color: "transparent" }
                    onTextChanged: debounce.restart()
                    Keys.onReturnPressed: {
                        debounce.stop();
                        var q = text.trim();
                        if (q.length < 2) return;
                        lastQuery = q;
                        searchResults = [];
                        loading = true;
                        backend.search(q);
                        pollTimer.start();
                    }
                }

                BusyIndicator {
                    running: loading
                    visible: loading
                    implicitWidth: 24; implicitHeight: 24
                }

                ToolButton {
                    text: "✕"
                    visible: searchField.text !== ""
                    onClicked: {
                        searchField.text = "";
                        lastQuery = "";
                        searchResults = [];
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: palette.mid
            opacity: 0.3
        }

        // ── Results ─────────────────────────────────────────────────────────
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            // Placeholder: type to search
            Label {
                anchors.centerIn: parent
                text: "Type to search for apps and packages"
                color: root.dimText
                font.pixelSize: 14
                visible: searchField.text.trim().length < 2 && !loading
            }

            // No results
            Label {
                anchors.centerIn: parent
                text: "No results for \"" + lastQuery + "\""
                color: root.dimText
                font.pixelSize: 14
                visible: !loading && lastQuery !== "" && searchResults.length === 0
            }

            ScrollView {
                anchors.fill: parent
                contentWidth: availableWidth
                visible: searchResults.length > 0
                clip: true

                Column {
                    width: parent.width
                    topPadding: 4
                    bottomPadding: 8

                    // Result count header
                    Label {
                        leftPadding: 16
                        topPadding: 8
                        bottomPadding: 4
                        text: searchResults.length + " result" + (searchResults.length !== 1 ? "s" : "") +
                              " for \"" + lastQuery + "\""
                        font.pixelSize: 12
                        color: root.dimText
                    }

                    Repeater {
                        model: searchResults

                        Rectangle {
                            width: searchPage.width
                            height: 56
                            color: rowMouse.containsMouse ? palette.highlight : "transparent"

                            RowLayout {
                                anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
                                spacing: 12

                                AppIcon {
                                    iconPath: modelData.icon_path || ""
                                    iconUrl: modelData.icon_url || ""
                                    iconName: modelData.name || modelData.id || "?"
                                    size: 36
                                }

                                Column {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Row {
                                        spacing: 6

                                        Label {
                                            text: modelData.name || modelData.id || ""
                                            font.bold: true
                                            color: rowMouse.containsMouse ? palette.highlightedText : palette.text
                                        }

                                        // Source badge
                                        Rectangle {
                                            height: 16
                                            width: srcLabel.implicitWidth + 10
                                            radius: 3
                                            color: root.sourceColor(modelData.source || "")
                                            anchors.verticalCenter: parent.verticalCenter

                                            Label {
                                                id: srcLabel
                                                anchors.centerIn: parent
                                                text: root.sourceLabel(modelData.source || "")
                                                font.pixelSize: 9
                                                color: "white"
                                            }
                                        }
                                    }

                                    Label {
                                        text: modelData.summary || ""
                                        font.pixelSize: 11
                                        color: rowMouse.containsMouse ? palette.highlightedText : root.dimText
                                        elide: Text.ElideRight
                                        width: parent.width
                                        visible: text !== ""
                                    }
                                }

                                Button {
                                    text: "Install"
                                    visible: !modelData.installed
                                    onClicked: backend.installApp(modelData.id || "", modelData.source || "", modelData.remote || "",
                                                                   modelData.name || "", modelData.icon_path || "", modelData.icon_url || "",
                                                                   modelData.user_remote === true)
                                }

                                Label {
                                    text: "✓ Installed"
                                    color: "#4caf50"
                                    font.pixelSize: 12
                                    visible: modelData.installed === true
                                }
                            }

                            Rectangle {
                                anchors { bottom: parent.bottom; left: parent.left; right: parent.right; leftMargin: 16; rightMargin: 16 }
                                height: 1
                                color: palette.mid
                                opacity: 0.15
                            }

                            MouseArea {
                                id: rowMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.showDetail(modelData)
                            }
                        }
                    }
                }
            }
        }
    }
}

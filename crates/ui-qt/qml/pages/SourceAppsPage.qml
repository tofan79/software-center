import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: sourcePage

    property string source: "dnf"
    property string titleText: "DNF Apps"
    property var    apps: []
    property bool   loading: false
    property int    visibleCount: 0

    property int viewMode: 0
    property int sortMode: 0

    readonly property int pageSize: 60

    function _sortedApps() {
        var arr = apps.slice();
        if (sortMode === 1) {
            arr.sort(function(a,b){ return (b.name || "").localeCompare(a.name || ""); });
        } else if (sortMode === 2) {
            arr.sort(function(a,b){ return (b.updated || "").localeCompare(a.updated || ""); });
        } else if (sortMode === 3) {
            arr.sort(function(a,b){
                var ai = (a.installed === true) ? 1 : 0;
                var bi = (b.installed === true) ? 1 : 0;
                if (ai !== bi) return bi - ai;
                return (a.name || "").localeCompare(b.name || "");
            });
        } else {
            arr.sort(function(a,b){ return (a.name || "").localeCompare(b.name || ""); });
        }
        return arr;
    }

    property var displayedApps: {
        var sorted = _sortedApps();
        return sorted.slice(0, Math.min(visibleCount, sorted.length));
    }

    function activate() {
        pollTimer.stop();
        apps = [];
        visibleCount = 0;
        loading = true;
        backend.loadSource(source);
        pollTimer.start();
    }

    function revealMore() {
        if (loading || apps.length === 0) return;
        if (visibleCount < apps.length)
            visibleCount = Math.min(apps.length, visibleCount + pageSize);
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
                    try {
                        apps = JSON.parse(backend.readLog());
                        visibleCount = Math.min(pageSize, apps.length);
                    }
                    catch(e) {
                        apps = [];
                    }
                }
            }
        }
    }

    // ── UI ─────────────────────────────────────────────────────────────────────

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Item {
            Layout.fillWidth: true
            height: 44

            Label {
                anchors { left: parent.left; leftMargin: 24; verticalCenter: parent.verticalCenter }
                text: sourcePage.titleText
                font.pixelSize: 18
                font.bold: true
            }
        }

        // ── View / sort toolbar ────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 40
            color: palette.button
            visible: !loading && apps.length > 0

            RowLayout {
                anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
                spacing: 10

                Label { text: "View:"; font.pixelSize: 12; color: root.dimText }

                Button {
                    text: "Grid"
                    font.pixelSize: 12
                    highlighted: sourcePage.viewMode === 0
                    flat: !(sourcePage.viewMode === 0)
                    implicitHeight: 26
                    onClicked: sourcePage.viewMode = 0
                }
                Button {
                    text: "List"
                    font.pixelSize: 12
                    highlighted: sourcePage.viewMode === 1
                    flat: !(sourcePage.viewMode === 1)
                    implicitHeight: 26
                    onClicked: sourcePage.viewMode = 1
                }

                Item { Layout.fillWidth: true }

                Label { text: "Sort:"; font.pixelSize: 12; color: root.dimText }

                ComboBox {
                    font.pixelSize: 12
                    implicitHeight: 26
                    implicitWidth: 150
                    model: ["Name (A–Z)", "Name (Z–A)", "Recently Updated", "Installed First"]
                    onActivated: sourcePage.sortMode = currentIndex
                }
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            clip: true

            Column {
                width: parent.width
                topPadding: 8
                bottomPadding: 24
                spacing: 0

                Item {
                    width: parent.width
                    height: 60
                    visible: loading

                    Row {
                        anchors.centerIn: parent
                        spacing: 12
                        BusyIndicator { running: loading; implicitWidth: 28; implicitHeight: 28 }
                        Label {
                            text: "Loading apps…"
                            anchors.verticalCenter: parent.verticalCenter
                        }
                    }
                }

                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    topPadding: 40
                    text: "No apps found for this source."
                    color: root.dimText
                    font.pixelSize: 14
                    visible: !loading && apps.length === 0
                }

                Flow {
                    width: parent.width - 32
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 12
                    visible: !loading && displayedApps.length > 0 && sourcePage.viewMode === 0
                    topPadding: 4

                    Repeater {
                        model: loading ? [] : sourcePage.displayedApps

                        Rectangle {
                            width: 120
                            height: 120
                            radius: 10
                            color: cardArea.containsMouse ? Qt.lighter(root.cardColor, 1.08) : root.cardColor
                            border.color: palette.mid
                            border.width: 1

                            Column {
                                anchors { fill: parent; margins: 8 }
                                spacing: 6

                                AppIcon {
                                    iconPath: modelData.icon_path || ""
                                    iconUrl: modelData.icon_url || ""
                                    iconName: modelData.name || modelData.id || "?"
                                    size: 56
                                    anchors.horizontalCenter: parent.horizontalCenter
                                }

                                Label {
                                    width: parent.width
                                    text: modelData.name || modelData.id || ""
                                    font.pixelSize: 11
                                    elide: Text.ElideRight
                                    horizontalAlignment: Text.AlignHCenter
                                }
                            }

                            Rectangle {
                                visible: modelData.installed === true
                                width: 16; height: 16
                                radius: 8
                                color: "#4caf50"
                                anchors { top: parent.top; right: parent.right; margins: 4 }
                                Label {
                                    anchors.centerIn: parent
                                    text: "✓"
                                    font.pixelSize: 9
                                    color: "white"
                                }
                            }

                            MouseArea {
                                id: cardArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.showDetail(modelData)
                            }
                        }
                    }
                }

                // ── App list (rows with detail) ────────────────────────────────
                Column {
                    width: parent.width
                    visible: !loading && displayedApps.length > 0 && sourcePage.viewMode === 1
                    topPadding: 4

                    Repeater {
                        model: loading ? [] : sourcePage.displayedApps

                        Rectangle {
                            width: parent.width
                            height: 56
                            color: listMouse.containsMouse ? palette.highlight : "transparent"

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
                                            color: listMouse.containsMouse ? palette.highlightedText : palette.text
                                        }

                                        Rectangle {
                                            height: 16
                                            width: srcLabel.implicitWidth + 10
                                            radius: 3
                                            color: root.sourceColor(modelData.source || "")
                                            anchors.verticalCenter: parent.verticalCenter
                                            visible: modelData.source !== ""

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
                                        width: parent.width
                                        text: modelData.summary || ""
                                        font.pixelSize: 11
                                        color: listMouse.containsMouse ? palette.highlightedText : root.dimText
                                        elide: Text.ElideRight
                                        visible: text !== ""
                                    }
                                }

                                Label {
                                    text: modelData.updated || ""
                                    font.pixelSize: 10
                                    color: root.dimText
                                    visible: modelData.updated !== ""
                                }

                                Button {
                                    text: "Install"
                                    visible: modelData.installed !== true
                                    implicitHeight: 28
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
                                id: listMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.showDetail(modelData)
                            }
                        }
                    }
                }

                Item {
                    width: parent.width
                    height: apps.length > displayedApps.length ? 64 : 0
                    visible: apps.length > displayedApps.length

                    Row {
                        anchors.centerIn: parent
                        spacing: 12

                        Button {
                            text: "Load More"
                            onClicked: sourcePage.revealMore()
                        }

                        Label {
                            text: displayedApps.length + " of " + apps.length + " apps"
                            color: root.dimText
                            font.pixelSize: 11
                            anchors.verticalCenter: parent.verticalCenter
                        }
                    }
                }
            }
        }
    }
}

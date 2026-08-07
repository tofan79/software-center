import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Dialogs
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: appImagesPage

    property var apps: []
    property bool loading: false
    property bool installing: false

    // file:// URL -> local filesystem path
    function urlToPath(url) {
        var s = String(url);
        if (s.indexOf("file://") === 0) {
            s = s.substring(7);
            try { s = decodeURIComponent(s); } catch(e) {}
        }
        return s;
    }

    function isInstallable(path) {
        var lower = path.toLowerCase();
        return lower.indexOf(".appimage") !== -1
            || lower.endsWith(".zip")
            || lower.endsWith(".tar.gz")
            || lower.endsWith(".tgz");
    }

    // Install a dropped/picked AppImage (or archive containing one).
    function installFromPath(path) {
        if (installing) return;
        if (!path) return;
        installing = true;
        statusLabel.text = "";
        statusLabel.color = palette.text;
        backend.installLocalFile(path, "appimage", "install", "");
        installTimer.start();
    }

    function refreshList() {
        apps = [];
        backend.loadInstalled();
        pollTimer.start();
    }

    function activate() {
        loading = true;
        apps = [];
        backend.loadInstalled();
        pollTimer.start();
    }

    Timer {
        id: pollTimer
        interval: 300
        repeat: true
        onTriggered: {
            backend.pollOp();
            if (!backend.opRunning) {
                pollTimer.stop();
                loading = false;
                if (backend.opResult === 1) {
                    try {
                        var all = JSON.parse(backend.readLog());
                        apps = all.filter(function(a) { return a.source === "appimage"; });
                    } catch(e) { apps = []; }
                }
            }
        }
    }

    Timer {
        id: installTimer
        interval: 250
        repeat: true
        onTriggered: {
            backend.pollOp();
            if (!backend.opRunning) {
                installTimer.stop();
                installing = false;
                if (backend.opResult === 1) {
                    statusLabel.text = "Installed successfully!";
                    statusLabel.color = "green";
                    refreshList();
                } else {
                    statusLabel.text = "Installation failed:";
                    statusLabel.color = "red";
                    logArea.text = backend.readLog();
                    logArea.visible = logArea.text !== "";
                }
            }
        }
    }

    FileDialog {
        id: pickDialog
        title: "Install AppImage"
        nameFilters: ["AppImage files (*.AppImage *.appimage *.AppImage.tar.gz *.AppImage.zip)", "All files (*)"]
        onAccepted: installFromPath(appImagesPage.urlToPath(selectedFile))
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Header ────────────────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: 48
            color: palette.button

            RowLayout {
                anchors { fill: parent; leftMargin: 16; rightMargin: 16 }

                Label {
                    text: "AppImages"
                    font.pixelSize: 16
                    font.bold: true
                    Layout.fillWidth: true
                }

                Label {
                    text: installing ? "Installing…" : "Drop a .AppImage file to install"
                    font.pixelSize: 11
                    color: root.dimText
                    visible: !installing
                }

                BusyIndicator {
                    visible: installing
                    running: installing
                    implicitWidth: 18
                    implicitHeight: 18
                }

                Button {
                    text: "📁  Install AppImage…"
                    font.pixelSize: 12
                    onClicked: pickDialog.open()
                }
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

        // ── Install status ────────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            color: "transparent"
            height: statusLabel.text === "" ? 0 : 44

            Label {
                id: statusLabel
                anchors { left: parent.left; leftMargin: 16; verticalCenter: parent.verticalCenter }
                text: ""
                font.bold: true
                visible: text !== ""
            }

            Label {
                id: logArea
                anchors { left: parent.left; leftMargin: 16; right: parent.right; rightMargin: 16; top: parent.top; topMargin: 34 }
                text: ""
                color: "red"
                font.pixelSize: 10
                font.family: "monospace"
                wrapMode: Text.WrapAnywhere
                visible: false
            }
        }

        // ── Loading spinner ───────────────────────────────────────────────────
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: loading

            Row {
                anchors.centerIn: parent
                spacing: 12
                BusyIndicator { running: loading; implicitWidth: 32; implicitHeight: 32 }
                Label { text: "Loading…"; anchors.verticalCenter: parent.verticalCenter }
            }
        }

        // ── App list ──────────────────────────────────────────────────────────
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            clip: true
            visible: !loading

            Column {
                width: parent.width
                topPadding: 4
                bottomPadding: 8

                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    topPadding: 40
                    text: "No AppImages installed.\nDrop a .AppImage file here, or click Browse."
                    horizontalAlignment: Text.AlignHCenter
                    color: root.dimText
                    visible: appImagesPage.apps.length === 0

                    Rectangle {
                        anchors { horizontalCenter: parent.horizontalCenter; top: parent.bottom; topMargin: 14 }
                        width: browseBtn.implicitWidth + 28
                        height: 30
                        radius: 6
                        color: root.cardColor
                        border.color: palette.mid
                        border.width: 1

                        Button {
                            id: browseBtn
                            anchors.centerIn: parent
                            text: "Browse files…"
                            flat: true
                            font.pixelSize: 12
                            onClicked: pickDialog.open()
                        }
                    }
                }

                Repeater {
                    model: appImagesPage.apps

                    Rectangle {
                        id: aiRow
                        width: appImagesPage.width
                        height: 60
                        color: aiArea.containsMouse
                            ? Qt.rgba(palette.highlight.r, palette.highlight.g, palette.highlight.b, 0.08)
                            : "transparent"

                        RowLayout {
                            anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
                            spacing: 12

                            AppIcon {
                                iconPath: modelData.icon_path || ""
                                iconUrl: ""
                                iconName: modelData.name || modelData.id || "?"
                                size: 36
                            }

                            Column {
                                Layout.fillWidth: true
                                spacing: 2

                                Label {
                                    text: modelData.name || modelData.id || ""
                                    font.bold: true
                                    elide: Text.ElideRight
                                    width: parent.width
                                }

                                Label {
                                    text: modelData.version
                                          ? "v" + modelData.version
                                          : (modelData.summary || "")
                                    font.pixelSize: 11
                                    color: root.dimText
                                    elide: Text.ElideRight
                                    width: parent.width
                                    visible: text !== ""
                                }
                            }

                            // Update source badge
                            Rectangle {
                                visible: modelData.update_source && modelData.update_source !== "none"
                                         && modelData.update_source !== ""
                                radius: 4
                                color: root.cardColor
                                border.color: palette.mid
                                border.width: 1
                                width: updateSrcLbl.implicitWidth + 10
                                height: updateSrcLbl.implicitHeight + 4

                                Label {
                                    id: updateSrcLbl
                                    anchors.centerIn: parent
                                    text: modelData.update_source || ""
                                    font.pixelSize: 10
                                    color: root.dimText
                                }
                            }
                        }

                        MouseArea {
                            id: aiArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.showDetail(modelData)
                        }

                        Rectangle {
                            anchors { bottom: parent.bottom; left: parent.left; right: parent.right; leftMargin: 16; rightMargin: 16 }
                            height: 1
                            color: palette.mid
                            opacity: 0.15
                        }
                    }
                }
            }
        }
    }

    // ── Drag & drop zone (whole page) ─────────────────────────────────────────
    DropArea {
        id: dropArea
        anchors.fill: parent
        enabled: !installing

        onEntered: {
            if (drag.hasUrls) {
                var p = appImagesPage.urlToPath(drag.urls[0]);
                if (appImagesPage.isInstallable(p)) {
                    drag.accepted = true;
                    hoverRect.visible = true;
                }
            }
        }
        onExited: hoverRect.visible = false
        onDropped: {
            hoverRect.visible = false;
            if (drop.hasUrls && drop.urls.length > 0) {
                var path = appImagesPage.urlToPath(drop.urls[0]);
                if (appImagesPage.isInstallable(path)) {
                    drop.accepted = true;
                    appImagesPage.installFromPath(path);
                }
            }
        }

        // Hover highlight overlay while dragging an AppImage over the page
        Rectangle {
            id: hoverRect
            anchors.fill: parent
            visible: false
            color: Qt.rgba(palette.highlight.r, palette.highlight.g, palette.highlight.b, 0.12)
            border.color: palette.highlight
            border.width: 2
            radius: 6

            Label {
                anchors.centerIn: parent
                text: "Drop to install AppImage"
                font.bold: true
                font.pixelSize: 15
                color: palette.highlight
            }
        }
    }
}

// pages/LocalFilePage.qml — Install a local .rpm / .flatpak / .flatpakref / .AppImage
import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: localFilePage

    property string filePath: ""
    property string fileKind: ""   // "rpm" | "flatpak" | "flatpakref" | "appimage"
    property var    appInfo: null

    signal backRequested()

    // Load info for the file and populate UI
    function activate(path, kind) {
        filePath = path;
        fileKind = kind;
        appInfo  = null;
        statusLabel.text = "";
        statusLabel.color = palette.text;
        installBtn.enabled = true;
        installBtn.text = "Install";
        var json = backend.getLocalFileInfo(path, kind);
        try {
            appInfo = JSON.parse(json);
        } catch(e) {
            appInfo = { name: path, summary: "Could not read file metadata", description: "" };
        }
        nameLabel.text    = appInfo.name    || path;
        versionLabel.text = appInfo.version || "";
        summaryLabel.text = appInfo.summary || "";
        descLabel.text    = appInfo.description || "";

        var kindStr = kind.toUpperCase();
        if (kind === "rpm")        kindStr = "RPM Package";
        else if (kind === "flatpak")    kindStr = "Flatpak Bundle";
        else if (kind === "flatpakref") kindStr = "Flatpak Ref";
        else if (kind === "appimage")   kindStr = "AppImage";
        kindBadge.text = kindStr;

        var action = appInfo.install_action || (appInfo.installed ? "reinstall" : "install");
        if (action === "reinstall") {
            installBtn.text    = "Reinstall";
            installBtn.enabled = true;
        } else if (action === "upgrade") {
            installBtn.text    = "Upgrade";
            installBtn.enabled = true;
        } else if (appInfo.installed) {
            installBtn.text    = "Already Installed";
            installBtn.enabled = false;
        } else {
            installBtn.text    = "Install";
            installBtn.enabled = true;
        }
    }

    // Poll for install completion
    Timer {
        id: pollTimer
        interval: 200
        repeat: true
        running: false
        onTriggered: {
            backend.pollOp();
            if (!backend.opRunning) {
                pollTimer.stop();
                if (backend.opResult === 1) {
                    var action = localFilePage.appInfo ? (localFilePage.appInfo.install_action || "install") : "install";
                    statusLabel.text  = action === "upgrade" ? "Upgraded successfully!"
                                      : action === "reinstall" ? "Reinstalled successfully!"
                                      : "Installed successfully!";
                    statusLabel.color = "green";
                    installBtn.text   = "Done";
                    installBtn.enabled = false;
                } else {
                    statusLabel.text  = "Installation failed. See log below.";
                    statusLabel.color = "red";
                    installBtn.enabled = true;
                    installBtn.text   = "Retry";
                }
                logArea.text = backend.readLog();
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 0
        spacing: 0

        // ── Header bar ─────────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: 56
            color: palette.window
            layer.enabled: true

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 16
                spacing: 8

                ToolButton {
                    icon.name: "go-previous-symbolic"
                    text: "Back"
                    display: AbstractButton.IconOnly
                    onClicked: localFilePage.backRequested()
                }

                Label {
                    text: "Install Local File"
                    font.pixelSize: 16
                    font.bold: true
                    Layout.fillWidth: true
                }
            }
        }

        // ── Content ────────────────────────────────────────────────────────
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth

            ColumnLayout {
                width: parent.width
                spacing: 16
                anchors.margins: 24

                // Top spacing
                Item { height: 8; Layout.fillWidth: true }

                // Hero row
                RowLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 24
                    Layout.rightMargin: 24
                    spacing: 20

                    // Icon placeholder
                    Rectangle {
                        width: 80; height: 80
                        radius: 12
                        color: Qt.rgba(palette.text.r, palette.text.g, palette.text.b, 0.08)

                        Label {
                            anchors.centerIn: parent
                            text: {
                                if (localFilePage.fileKind === "rpm")        return "📦";
                                if (localFilePage.fileKind === "flatpak")    return "📦";
                                if (localFilePage.fileKind === "flatpakref") return "🔗";
                                if (localFilePage.fileKind === "appimage")   return "🖼";
                                return "📄";
                            }
                            font.pixelSize: 36
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4

                        Label {
                            id: nameLabel
                            text: ""
                            font.pixelSize: 22
                            font.bold: true
                            wrapMode: Text.WordWrap
                            Layout.fillWidth: true
                        }

                        Label {
                            id: versionLabel
                            text: ""
                            color: root.dimText
                            font.pixelSize: 12
                        }

                        Rectangle {
                            id: kindBadge
                            property alias text: badgeLabel.text
                            height: 22
                            width: badgeLabel.implicitWidth + 16
                            radius: 4
                            color: {
                                if (localFilePage.fileKind === "flatpak" || localFilePage.fileKind === "flatpakref")
                                    return "#1565c0";
                                if (localFilePage.fileKind === "appimage") return "#6a1b9a";
                                return "#37474f";
                            }
                            Label {
                                id: badgeLabel
                                anchors.centerIn: parent
                                color: "white"
                                font.pixelSize: 11
                                font.bold: true
                            }
                        }
                    }

                    Button {
                        id: installBtn
                        text: "Install"
                        font.bold: true
                        highlighted: true
                        onClicked: {
                            installBtn.enabled = false;
                            installBtn.text = "Installing…";
                            statusLabel.text = "";
                            logArea.text = "";
                            var action   = localFilePage.appInfo ? (localFilePage.appInfo.install_action || "install") : "install";
                            var pkgName  = localFilePage.appInfo ? (localFilePage.appInfo.name || "") : "";
                            backend.installLocalFile(localFilePage.filePath, localFilePage.fileKind, action, pkgName);
                            pollTimer.start();
                        }
                    }
                }

                // Path label
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 24
                    Layout.rightMargin: 24
                    text: localFilePage.filePath
                    color: root.dimText
                    font.pixelSize: 11
                    elide: Text.ElideLeft
                    wrapMode: Text.WrapAnywhere
                }

                // Status
                Label {
                    id: statusLabel
                    Layout.fillWidth: true
                    Layout.leftMargin: 24
                    Layout.rightMargin: 24
                    text: ""
                    font.bold: true
                    wrapMode: Text.WordWrap
                    visible: text !== ""
                }

                // Progress bar while installing
                ProgressBar {
                    Layout.fillWidth: true
                    Layout.leftMargin: 24
                    Layout.rightMargin: 24
                    indeterminate: true
                    visible: backend.opRunning && pollTimer.running
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.leftMargin: 24
                    Layout.rightMargin: 24
                    height: 1
                    color: Qt.rgba(palette.text.r, palette.text.g, palette.text.b, 0.12)
                    visible: summaryLabel.text !== "" || descLabel.text !== ""
                }

                // Summary
                Label {
                    id: summaryLabel
                    Layout.fillWidth: true
                    Layout.leftMargin: 24
                    Layout.rightMargin: 24
                    text: ""
                    wrapMode: Text.WordWrap
                    font.pixelSize: 14
                    visible: text !== ""
                }

                // Description
                Label {
                    id: descLabel
                    Layout.fillWidth: true
                    Layout.leftMargin: 24
                    Layout.rightMargin: 24
                    text: ""
                    wrapMode: Text.WordWrap
                    color: root.dimText
                    visible: text !== ""
                }

                // Log output
                GroupBox {
                    title: "Install Log"
                    Layout.fillWidth: true
                    Layout.leftMargin: 24
                    Layout.rightMargin: 24
                    visible: logArea.text !== ""

                    ScrollView {
                        width: parent.width
                        height: 180
                        ScrollBar.horizontal.policy: ScrollBar.AsNeeded

                        TextArea {
                            id: logArea
                            readOnly: true
                            font.family: "monospace"
                            font.pixelSize: 11
                            wrapMode: Text.WrapAnywhere
                            background: Rectangle {
                                color: Qt.rgba(0, 0, 0, 0.04)
                                radius: 4
                            }
                        }
                    }
                }

                Item { height: 24; Layout.fillWidth: true }
            }
        }
    }
}

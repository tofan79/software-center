import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: appImagesPage

    property var apps: []
    property bool loading: false

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
                    text: "Drop a .AppImage file to install"
                    font.pixelSize: 11
                    color: root.dimText
                }
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

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
                    text: "No AppImages installed.\nDrop a .AppImage file to get started."
                    horizontalAlignment: Text.AlignHCenter
                    color: root.dimText
                    visible: appImagesPage.apps.length === 0
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
}

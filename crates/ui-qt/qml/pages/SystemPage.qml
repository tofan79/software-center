import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: systemPage

    property var statusData: null
    property bool loading: false

    function activate() {
        if (statusData === null) loadStatus();
    }

    function loadStatus() {
        loading = true;
        statusData = null;
        backend.loadSystemStatus();
        pollTimer.start();
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
                    try { statusData = JSON.parse(backend.readLog()); }
                    catch(e) { statusData = {}; }
                }
            }
        }
    }

    ScrollView {
        anchors.fill: parent
        contentWidth: availableWidth
        clip: true

        Column {
            width: parent.width
            spacing: 12
            topPadding: 20
            leftPadding: 20
            rightPadding: 20
            bottomPadding: 20

            // Page title
            Label {
                text: "System"
                font.pixelSize: 22
                font.bold: true
                bottomPadding: 4
            }

            // Loading
            Item {
                width: parent.width - 40
                height: 60
                visible: loading

                Row {
                    anchors.centerIn: parent
                    spacing: 12
                    BusyIndicator { running: loading; implicitWidth: 28; implicitHeight: 28 }
                    Label { text: "Loading system info…"; anchors.verticalCenter: parent.verticalCenter }
                }
            }

            // ── System info card ─────────────────────────────────────────────
            Rectangle {
                width: parent.width - 40
                height: systemInfoLayout.implicitHeight + 32
                radius: 8
                color: root.cardColor
                border.color: palette.mid
                border.width: 1
                visible: statusData !== null

                ColumnLayout {
                    id: systemInfoLayout
                    anchors { fill: parent; margins: 16 }
                    spacing: 10

                    Label {
                        text: "System Information"
                        font.pixelSize: 15
                        font.bold: true
                    }

                    Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

                    Repeater {
                        model: {
                            if (!statusData) return [];
                            return [
                                { label: "Operating System", value: statusData.os || "—" },
                                { label: "Version",         value: statusData.version || "—" },
                                { label: "Kernel",          value: statusData.kernel || "—" },
                            ];
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 12

                            Label {
                                text: modelData.label
                                color: root.dimText
                                font.pixelSize: 12
                                Layout.preferredWidth: 150
                            }
                            Label {
                                text: modelData.value
                                font.pixelSize: 12
                                font.bold: true
                                Layout.fillWidth: true
                                wrapMode: Text.WordWrap
                            }
                        }
                    }

                    Label {
                        text: statusData && statusData.error ? statusData.error : ""
                        color: "#e53935"
                        font.pixelSize: 12
                        visible: text !== ""
                        wrapMode: Text.WordWrap
                        Layout.fillWidth: true
                    }
                }
            }
        }
    }
}

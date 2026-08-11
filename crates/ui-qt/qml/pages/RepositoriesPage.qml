import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: reposPage

    property var coprRepos: []
    property var systemRepos: []
    property var fedoraRepos: []
    property bool loading: false
    property bool opActive: false

    function activate() {
        loadAll();
    }

    function loadAll() {
        loading = true;
        loadRepos();
    }

    function loadRepos() {
        backend.loadRepos();
        reposPollTimer.start();
    }

    Timer {
        id: reposPollTimer
        interval: 250
        repeat: true
        onTriggered: {
            backend.pollRepos();
            if (backend.reposReady) {
                reposPollTimer.stop();
                loading = false;
                var list = [];
                try { list = JSON.parse(backend.readRepos()); } catch(e) {}
                var copr = [], sys = [], fed = [];
                for (var i = 0; i < list.length; i++) {
                    var r = list[i];
                    if (r.kind === "copr")      copr.push(r);
                    else if (r.kind === "fedora") fed.push(r);
                    else                        sys.push(r);
                }
                coprRepos   = copr;
                systemRepos = sys;
                fedoraRepos = fed;
            }
        }
    }

    function toggleRepo(id, enabled) {
        opActive = true;
        statusLabel.text = enabled ? "Enabling " + id + "…" : "Disabling " + id + "…";
        statusLabel.color = palette.text;
        logArea.text = "";
        logArea.visible = false;
        backend.setRepoEnabled(id, enabled);
        opTimer.start();
    }

    function addCopr() {
        var spec = coprInput.text.trim();
        if (spec === "") return;
        opActive = true;
        statusLabel.text = "Adding COPR " + spec + "…";
        statusLabel.color = palette.text;
        logArea.text = "";
        logArea.visible = false;
        backend.addCopr(spec);
        opTimer.start();
    }

    function removeCopr(owner_project) {
        opActive = true;
        statusLabel.text = "Removing COPR " + owner_project + "…";
        statusLabel.color = palette.text;
        logArea.text = "";
        logArea.visible = false;
        backend.removeCopr(owner_project);
        opTimer.start();
    }

    Timer {
        id: opTimer
        interval: 300
        repeat: true
        onTriggered: {
            backend.pollOp();
            if (!backend.opRunning) {
                opTimer.stop();
                opActive = false;
                if (backend.opResult === 1) {
                    statusLabel.text = "Done.";
                    statusLabel.color = "green";
                } else {
                    statusLabel.text = "Operation failed:";
                    statusLabel.color = "red";
                    logArea.text = backend.readLog();
                    logArea.visible = logArea.text !== "";
                }
                loadAll();
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
                    text: "Repositories"
                    font.pixelSize: 16
                    font.bold: true
                    Layout.fillWidth: true
                }

                Button {
                    text: "↻  Refresh"
                    font.pixelSize: 12
                    flat: true
                    onClicked: loadAll()
                }
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

        // ── Status ────────────────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: statusLabel.text === "" && !opActive ? 0 : 40
            color: "transparent"
            visible: statusLabel.text !== "" || opActive

            RowLayout {
                anchors { left: parent.left; leftMargin: 16; right: parent.right; rightMargin: 16; verticalCenter: parent.verticalCenter }
                spacing: 8

                BusyIndicator {
                    visible: opActive
                    running: opActive
                    implicitWidth: 16; implicitHeight: 16
                }

                Label {
                    id: statusLabel
                    text: ""
                    font.bold: true
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
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

        // ── Content ───────────────────────────────────────────────────────────
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            clip: true

            Column {
                width: parent.width
                topPadding: 8
                bottomPadding: 16

                // ── Add COPR row ──────────────────────────────────────────────
                Rectangle {
                    width: parent.width - 32
                    anchors.horizontalCenter: parent.horizontalCenter
                    height: 44
                    radius: 6
                    color: root.cardColor
                    border.color: palette.mid
                    border.width: 1

                    RowLayout {
                        anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
                        spacing: 8

                        Label {
                            text: "Add COPR:"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        TextField {
                            id: coprInput
                            Layout.fillWidth: true
                            placeholderText: "owner/project  (e.g. tofan79/software-center)"
                            font.pixelSize: 12
                            selectByMouse: true
                        }

                        Button {
                            text: "Add"
                            font.bold: true
                            highlighted: true
                            implicitWidth: 70
                            onClicked: {
                                addCopr();
                                coprInput.text = "";
                            }
                        }
                    }
                }

                Item { width: parent.width; height: 14 }

                // ── COPR repos ────────────────────────────────────────────────
                SectionHeader { text: "COPR Repositories (" + coprRepos.length + ")" }

                Repeater {
                    model: reposPage.coprRepos
                    delegate: RepoRow { repo: modelData }
                }

                Item { width: parent.width; height: 14 }

                // ── System repos ──────────────────────────────────────────────
                SectionHeader { text: "Third-party Repositories (" + systemRepos.length + ")" }

                Repeater {
                    model: reposPage.systemRepos
                    delegate: RepoRow { repo: modelData }
                }

                Item { width: parent.width; height: 14 }

                // ── Fedora official ───────────────────────────────────────────
                SectionHeader { text: "Fedora Official (" + fedoraRepos.length + ")" }

                Repeater {
                    model: reposPage.fedoraRepos
                    delegate: RepoRow { repo: modelData }
                }

                Item { width: parent.width; height: 20 }
            }
        }
    }

    // ── Reusable repo row ──────────────────────────────────────────────────────
    component RepoRow: Rectangle {
        id: row
        property var repo: null

        width: parent.width
        height: 52
        color: rowArea.containsMouse
            ? Qt.rgba(palette.highlight.r, palette.highlight.g, palette.highlight.b, 0.06)
            : "transparent"

        RowLayout {
            anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
            spacing: 10

            // Repo kind badge
            Rectangle {
                width: 54
                height: 22
                radius: 4
                color: {
                    if (repo.kind === "copr")  return "#6a1b9a";
                    if (repo.kind === "fedora") return "#1565c0";
                    return "#37474f";
                }
                Label {
                    anchors.centerIn: parent
                    text: {
                        if (repo.kind === "copr")  return "COPR";
                        if (repo.kind === "fedora") return "Fedora";
                        return "3rd-party";
                    }
                    color: "white"
                    font.pixelSize: 9
                    font.bold: true
                }
            }

            Column {
                Layout.fillWidth: true
                spacing: 2

                Label {
                    text: repo.name || repo.id || ""
                    font.bold: true
                    font.pixelSize: 13
                    elide: Text.ElideRight
                    width: parent.width
                }

                Label {
                    text: repo.kind === "copr"
                          ? (repo.owner + "/" + repo.project)
                          : (repo.id || "")
                    font.pixelSize: 10
                    color: root.dimText
                    elide: Text.ElideRight
                    width: parent.width
                }
            }

            // Status pill
            Rectangle {
                width: 58
                height: 20
                radius: 10
                color: repo.enabled ? Qt.rgba(0.2, 0.7, 0.3, 0.15) : Qt.rgba(0.7, 0.2, 0.2, 0.15)
                Label {
                    anchors.centerIn: parent
                    text: repo.enabled ? "Enabled" : "Disabled"
                    font.pixelSize: 9
                    color: repo.enabled ? "#2e7d32" : "#c62828"
                }
            }

            // Enable/disable toggle
            Button {
                text: repo.enabled ? "Disable" : "Enable"
                font.pixelSize: 12
                flat: true
                implicitWidth: 72
                implicitHeight: 30
                enabled: !opActive
                onClicked: reposPage.toggleRepo(repo.id, !repo.enabled)
            }

            // Remove (COPR only)
            Button {
                visible: repo.kind === "copr"
                text: "Remove"
                font.pixelSize: 12
                flat: true
                implicitWidth: 72
                implicitHeight: 30
                enabled: !opActive
                contentItem: Label {
                    text: "Remove"
                    color: "#e53935"
                    font.pixelSize: 12
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                onClicked: reposPage.removeCopr(repo.owner + "/" + repo.project)
            }
        }

        MouseArea {
            id: rowArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
        }

        Rectangle {
            anchors { bottom: parent.bottom; left: parent.left; right: parent.right; leftMargin: 16; rightMargin: 16 }
            height: 1
            color: palette.mid
            opacity: 0.15
        }
    }

    // ── Section header ─────────────────────────────────────────────────────────
    component SectionHeader: Item {
        property string text: ""
        width: parent.width
        height: 26

        Label {
            anchors.verticalCenter: parent.verticalCenter
            leftPadding: 16
            text: parent.text
            font.bold: true
            font.pixelSize: 11
            color: root.dimText
        }
    }
}

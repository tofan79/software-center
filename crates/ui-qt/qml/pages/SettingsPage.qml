import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: settingsPage

    property var settings: ({
        update_interval: 1440,
        auto_check_packages: true,
        auto_check_flatpak: true,
        auto_check_appimages: true,
        auto_update: false,
    })

    property bool saved: false
    property int maintUnusedCount: 0
    property bool maintActive: false

    function activate() {
        loadSettings();
        loadMaintUnused();
        reposTab.loadRepos();
    }

    function loadMaintUnused() {
        backend.loadUnusedPackages();
        unusedPollTimer.start();
    }

    Timer {
        id: unusedPollTimer
        interval: 250
        repeat: true
        onTriggered: {
            backend.pollUnused();
            if (backend.unusedReady) {
                unusedPollTimer.stop();
                try { maintUnusedCount = JSON.parse(backend.readUnused()).length; }
                catch(e) { maintUnusedCount = 0; }
            }
        }
    }

    function maintRun(which) {
        maintActive = true;
        maintStatus.text = "";
        maintLog.text = "";
        maintLog.visible = false;
        switch (which) {
            case "dnf_unused":
                maintStatus.text = "Removing unused packages…";
                backend.removeUnusedPackages();
                break;
            case "dnf_cache":
                maintStatus.text = "Clearing dnf cache…";
                backend.cleanDnfCache();
                break;
            case "flatpak":
                maintStatus.text = "Cleaning unused Flatpak runtimes…";
                backend.cleanFlatpakUnused();
                break;
            case "appimage":
                maintStatus.text = "Cleaning orphaned AppImage files…";
                backend.cleanAppImageCache();
                break;
        }
        maintTimer.start();
    }

    Timer {
        id: maintTimer
        interval: 300
        repeat: true
        onTriggered: {
            backend.pollOp();
            if (!backend.opRunning) {
                maintTimer.stop();
                maintActive = false;
                if (backend.opResult === 1) {
                    maintStatus.text = "Done.";
                    maintStatus.color = "#4caf50";
                } else {
                    maintStatus.text = "Operation failed:";
                    maintStatus.color = "#e53935";
                    maintLog.text = backend.readLog();
                    maintLog.visible = maintLog.text !== "";
                }
                loadMaintUnused();
            }
        }
    }

    function loadSettings() {
        try {
            var json = backend.loadSettings();
            var s = JSON.parse(json);
            settings = s;
            applySettings();
        } catch(e) {}
    }

    function applySettings() {
        intervalCombo.currentIndex = {
            360: 0, 720: 1, 1440: 2, 10080: 3, 0: 4
        }[settings.update_interval] || 2;

        checkPkgs.checked     = settings.auto_check_packages !== false;
        checkFlatpak.checked  = settings.auto_check_flatpak  !== false;
        checkAI.checked       = settings.auto_check_appimages !== false;
        autoUpdate.checked    = settings.auto_update === true;
    }

    function saveSettings() {
        var s = {
            update_interval:      [360, 720, 1440, 10080, 0][intervalCombo.currentIndex] || 1440,
            auto_check_packages:  checkPkgs.checked,
            auto_check_flatpak:   checkFlatpak.checked,
            auto_check_appimages: checkAI.checked,
            auto_update:          autoUpdate.checked,
        };
        backend.saveSettings(JSON.stringify(s));
        saved = true;
        savedTimer.restart();
    }

    Timer {
        id: savedTimer
        interval: 2000
        onTriggered: saved = false
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        TabBar {
            id: settingsTab
            Layout.fillWidth: true

            TabButton { text: "🔄  Updates" }
            TabButton { text: "📦  Flatpak Repositories"; onClicked: flatpakTab.loadRemotes() }
            TabButton { text: "🗂  Repositories";         onClicked: reposTab.activate() }
            TabButton { text: "⚙️   System";             onClicked: systemTab.activate() }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: settingsTab.currentIndex

            // ── Updates tab ──────────────────────────────────────────────────
            ScrollView {
                contentWidth: availableWidth
                clip: true

                Column {
                    width: parent.width
                    spacing: 0
                    topPadding: 20
                    leftPadding: 24
                    rightPadding: 24
                    bottomPadding: 20

                    Label {
                        text: "Update Schedule"
                        font.pixelSize: 16
                        font.bold: true
                        bottomPadding: 4
                    }

                    Label {
                        text: "How often Software Center checks for available updates in the background."
                        color: root.dimText
                        font.pixelSize: 12
                        wrapMode: Text.WordWrap
                        width: parent.width
                        bottomPadding: 16
                    }

                    RowLayout {
                        width: parent.width
                        spacing: 12

                        Label { text: "Check for updates:" }

                        ComboBox {
                            id: intervalCombo
                            model: ["Every 6 hours", "Every 12 hours", "Daily", "Weekly", "Manual only"]
                            currentIndex: 2
                            width: 200
                        }

                        Item { Layout.fillWidth: true }
                    }

                    Item { height: 16; width: 1 }
                    Rectangle { width: parent.width; height: 1; color: palette.mid; opacity: 0.3 }
                    Item { height: 16; width: 1 }

                    Label {
                        text: "Check for"
                        font.pixelSize: 14
                        font.bold: true
                        bottomPadding: 8
                    }

                    CheckBox { id: checkPkgs;    text: "Package updates (DNF)";        checked: true }
                    CheckBox { id: checkFlatpak; text: "Flatpak updates";               checked: true }
                    CheckBox { id: checkAI;      text: "AppImage updates";              checked: true }

                    Item { height: 16; width: 1 }
                    Rectangle { width: parent.width; height: 1; color: palette.mid; opacity: 0.3 }
                    Item { height: 16; width: 1 }

                    Label {
                        text: "Automatic Updates"
                        font.pixelSize: 14
                        font.bold: true
                        bottomPadding: 8
                    }

                    CheckBox {
                        id: autoUpdate
                        text: "Automatically install package and Flatpak updates when found"
                        checked: false
                    }

                    Label {
                        text: "Package and Flatpak updates may also be applied manually from the Updates page."
                        color: root.dimText
                        font.pixelSize: 11
                        wrapMode: Text.WordWrap
                        width: parent.width
                        topPadding: 4
                    }

                    Item { height: 16; width: 1 }
                    Rectangle { width: parent.width; height: 1; color: palette.mid; opacity: 0.3 }
                    Item { height: 16; width: 1 }

                    Label {
                        text: "Cache Maintenance"
                        font.pixelSize: 14
                        font.bold: true
                        bottomPadding: 8
                    }

                    Label {
                        text: "Clean leftover DNF metadata, unused dependency packages, unused Flatpak runtimes, and orphaned AppImage files."
                        color: root.dimText
                        font.pixelSize: 11
                        wrapMode: Text.WordWrap
                        width: parent.width
                        bottomPadding: 12
                    }

                    GridLayout {
                        columns: 2
                        columnSpacing: 8
                        rowSpacing: 8
                        width: parent.width

                        Button {
                            text: "🗑  Remove " + settingsPage.maintUnusedCount + " unused packages (DNF)"
                            enabled: settingsPage.maintUnusedCount > 0 && !settingsPage.maintActive
                            onClicked: settingsPage.maintRun("dnf_unused")
                        }
                        Button {
                            text: "🧹  Clear dnf cache"
                            enabled: !settingsPage.maintActive
                            onClicked: settingsPage.maintRun("dnf_cache")
                        }
                        Button {
                            text: "🧽  Clean unused Flatpak runtimes"
                            enabled: !settingsPage.maintActive
                            onClicked: settingsPage.maintRun("flatpak")
                        }
                        Button {
                            text: "📦  Clean orphaned AppImage files"
                            enabled: !settingsPage.maintActive
                            onClicked: settingsPage.maintRun("appimage")
                        }
                    }

                    Label {
                        id: maintStatus
                        text: ""
                        color: "#4caf50"
                        font.pixelSize: 12
                        wrapMode: Text.WordWrap
                        width: parent.width
                        topPadding: 4
                        visible: text !== ""
                    }

                    Label {
                        id: maintLog
                        text: ""
                        color: "#e53935"
                        font.pixelSize: 10
                        font.family: "monospace"
                        wrapMode: Text.WrapAnywhere
                        width: parent.width
                        visible: text !== ""
                    }

                    Item { height: 24; width: 1 }

                    RowLayout {
                        width: parent.width
                        spacing: 12

                        Button {
                            text: "Save Settings"
                            highlighted: true
                            onClicked: saveSettings()
                        }

                        Label {
                            text: "✓ Settings saved"
                            color: "#4caf50"
                            font.pixelSize: 12
                            visible: saved
                        }

                        Item { Layout.fillWidth: true }
                    }
                }
            }

            // ── Flatpak repos tab ────────────────────────────────────────────
            Item {
                id: flatpakTab

                property var remotes: []
                property bool hasFlathub: false
                property bool hasFlathubSystem: false
                property bool hasFlathubUser: false
                property bool hasCosmicWelcome: false
                property bool hasCosmicRemoteSystem: false
                property bool hasCosmicRemoteUser: false
                property string statusMsg: ""
                property bool statusOk: true

                function loadRemotes() {
                    backend.loadFlatpakRemotes();
                    remotesPollTimer.start();
                }

                Timer {
                    id: remotesPollTimer
                    interval: 250
                    repeat: true
                    onTriggered: {
                        backend.pollRemotes();
                        if (backend.remotesReady) {
                            remotesPollTimer.stop();
                            try {
                                var json = backend.readRemotes();
                                var data = JSON.parse(json);
                                remotes = data.remotes || [];
                                hasFlathub = data.has_flathub === true;
                                hasFlathubSystem = data.has_flathub_system === true;
                                hasFlathubUser = data.has_flathub_user === true;
                                hasCosmicWelcome = data.has_cosmic_welcome === true;
                                hasCosmicRemoteSystem = data.has_cosmic_remote_system === true;
                                hasCosmicRemoteUser = data.has_cosmic_remote_user === true;
                            } catch(e) {
                                remotes = [];
                                hasFlathub = false;
                                hasFlathubSystem = false;
                                hasFlathubUser = false;
                                hasCosmicWelcome = false;
                                hasCosmicRemoteSystem = false;
                                hasCosmicRemoteUser = false;
                            }
                        }
                    }
                }

                ColumnLayout {
                    anchors.fill: parent
                    spacing: 0

                    // Header row
                    Rectangle {
                        Layout.fillWidth: true
                        height: hdrRow.implicitHeight + 24
                        color: root.cardColor

                        RowLayout {
                            id: hdrRow
                            anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
                            spacing: 10

                            Column {
                                Layout.fillWidth: true
                                Label {
                                    text: "Flatpak Repositories"
                                    font.pixelSize: 15
                                    font.bold: true
                                }
                                Label {
                                    text: "Manage Flatpak repositories. System remotes are available to all users."
                                    font.pixelSize: 11
                                    color: root.dimText
                                    wrapMode: Text.WordWrap
                                    width: parent.width
                                }
                            }

                            Button {
                                text: "➕  Add Flathub (System)"
                                visible: !flatpakTab.hasFlathubSystem
                                onClicked: {
                                    var res = JSON.parse(backend.addFlathub(true));
                                    flatpakTab.statusMsg = res.msg || "";
                                    flatpakTab.statusOk  = res.ok === true;
                                    flatpakTab.loadRemotes();
                                }
                            }

                            Button {
                                text: "➕  Add Flathub (User)"
                                visible: !flatpakTab.hasFlathubUser
                                onClicked: {
                                    var res = JSON.parse(backend.addFlathub(false));
                                    flatpakTab.statusMsg = res.msg || "";
                                    flatpakTab.statusOk  = res.ok === true;
                                    flatpakTab.loadRemotes();
                                }
                            }

                            Button {
                                text: "➕  Add COSMIC (System)"
                                visible: flatpakTab.hasCosmicWelcome && !flatpakTab.hasCosmicRemoteSystem
                                onClicked: {
                                    var res = JSON.parse(backend.addCosmicRemote(true));
                                    flatpakTab.statusMsg = res.msg || "";
                                    flatpakTab.statusOk  = res.ok === true;
                                    flatpakTab.loadRemotes();
                                }
                            }

                            Button {
                                text: "➕  Add COSMIC (User)"
                                visible: flatpakTab.hasCosmicWelcome && !flatpakTab.hasCosmicRemoteUser
                                onClicked: {
                                    var res = JSON.parse(backend.addCosmicRemote(false));
                                    flatpakTab.statusMsg = res.msg || "";
                                    flatpakTab.statusOk  = res.ok === true;
                                    flatpakTab.loadRemotes();
                                }
                            }

                            Button {
                                text: "➕  Add Remote"
                                onClicked: addRemoteDlg.open()
                            }
                        }
                    }

                    Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

                    Label {
                        leftPadding: 16
                        topPadding: 8
                        bottomPadding: 4
                        text: flatpakTab.statusMsg
                        color: flatpakTab.statusOk ? "#4caf50" : "#e53935"
                        font.pixelSize: 12
                        visible: flatpakTab.statusMsg !== ""
                        Layout.fillWidth: true
                    }

                    ScrollView {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        contentWidth: availableWidth
                        clip: true

                        Column {
                            width: parent.width
                            topPadding: 8
                            bottomPadding: 8
                            spacing: 8

                            // Empty state
                            Label {
                                anchors.horizontalCenter: parent.horizontalCenter
                                topPadding: 40
                                text: "No Flatpak remotes configured."
                                color: root.dimText
                                visible: flatpakTab.remotes.length === 0
                            }

                            Repeater {
                                model: flatpakTab.remotes

                                Rectangle {
                                    id: remoteRow
                                    width: parent.width - 32
                                    anchors.horizontalCenter: parent.horizontalCenter
                                    height: remoteRowLayout.implicitHeight + 20
                                    radius: 6
                                    color: root.cardColor
                                    border.color: palette.mid
                                    border.width: 1

                                    property string rowStatusMsg: ""
                                    property bool rowStatusOk: true

                                    RowLayout {
                                        id: remoteRowLayout
                                        anchors { fill: parent; leftMargin: 14; rightMargin: 14 }
                                        spacing: 10

                                        CheckBox {
                                            checked: modelData.enabled
                                            onToggled: {
                                                var res = JSON.parse(backend.setFlatpakRemoteEnabled(
                                                    modelData.name, checked, modelData.system));
                                                remoteRow.rowStatusMsg = res.msg || "";
                                                remoteRow.rowStatusOk = res.ok === true;
                                                if (!res.ok) { checked = !checked; }
                                            }
                                        }

                                        Column {
                                            Layout.fillWidth: true
                                            spacing: 2

                                            RowLayout {
                                                spacing: 8
                                                Label {
                                                    text: modelData.title || modelData.name
                                                    font.bold: true
                                                    font.pixelSize: 13
                                                }
                                                Rectangle {
                                                    radius: 3
                                                    color: modelData.system ? "#1a237e" : "#37474f"
                                                    width: scopeLbl.implicitWidth + 8
                                                    height: 16
                                                    Label {
                                                        id: scopeLbl
                                                        anchors.centerIn: parent
                                                        text: modelData.system ? "system" : "user"
                                                        font.pixelSize: 9
                                                        color: "white"
                                                    }
                                                }
                                            }

                                            Label {
                                                text: modelData.url || ""
                                                font.pixelSize: 11
                                                color: root.dimText
                                                visible: text !== ""
                                                elide: Text.ElideRight
                                                width: parent.width
                                            }

                                            Label {
                                                text: remoteRow.rowStatusMsg
                                                font.pixelSize: 11
                                                color: remoteRow.rowStatusOk ? "#4caf50" : "#e53935"
                                                visible: text !== ""
                                            }
                                        }

                                        Button {
                                            flat: true
                                            implicitWidth: 72; implicitHeight: 30
                                            contentItem: Label {
                                                text: "Remove"
                                                color: "#e53935"
                                                font.pixelSize: 12
                                                horizontalAlignment: Text.AlignHCenter
                                                verticalAlignment: Text.AlignVCenter
                                            }
                                            onClicked: {
                                                var res = JSON.parse(backend.removeFlatpakRemote(
                                                    modelData.name, modelData.system));
                                                flatpakTab.statusMsg = res.msg || "";
                                                flatpakTab.statusOk  = res.ok === true;
                                                flatpakTab.loadRemotes();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // ── Add remote dialog ─────────────────────────────────────────
                Dialog {
                    id: addRemoteDlg
                    title: "Add Flatpak Remote"
                    modal: true
                    width: 420
                    standardButtons: Dialog.Ok | Dialog.Cancel

                    onAccepted: {
                        var name = addNameField.text.trim();
                        var url  = addUrlField.text.trim();
                        if (!name || !url) {
                            addDlgStatus.text = "Name and URL are required.";
                            addDlgStatus.color = "#e53935";
                            return;
                        }
                        var res = JSON.parse(backend.addFlatpakRemote(name, url, addSystemChk.checked));
                        if (res.ok) {
                            flatpakTab.loadRemotes();
                            addNameField.text = "";
                            addUrlField.text  = "";
                        } else {
                            addDlgStatus.text  = res.msg || "Failed.";
                            addDlgStatus.color = "#e53935";
                        }
                    }

                    Column {
                        width: parent.width
                        spacing: 10

                        Label { text: "Name (e.g. flathub):" }
                        TextField {
                            id: addNameField
                            width: parent.width
                            placeholderText: "remote-name"
                        }

                        Label { text: "URL (.flatpakrepo or repository URL):" }
                        TextField {
                            id: addUrlField
                            width: parent.width
                            placeholderText: "https://…"
                        }

                        CheckBox {
                            id: addSystemChk
                            text: "Install as system-wide remote (recommended)"
                            checked: true
                        }

                        Label {
                            id: addDlgStatus
                            text: ""
                            wrapMode: Text.WordWrap
                            width: parent.width
                            visible: text !== ""
                        }
                    }
                }

                Component.onCompleted: loadRemotes()
            }

            // ── DNF / COPR repositories tab ──────────────────────────────────
            RepositoriesPage {
                id: reposTab
            }

            // ── System tab ───────────────────────────────────────────────────
            SystemPage {
                id: systemTab
            }
        }
    }

    Component.onCompleted: {
        loadSettings();
    }
}

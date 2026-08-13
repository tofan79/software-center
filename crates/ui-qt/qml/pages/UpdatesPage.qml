import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: updatesPage

    property var updateData: null
    property bool checking: false
    property bool updating: false
    property bool rebootRequired: false

    // Update queue: ["packages", "flatpak"] in order
    property var updateQueue: []
    property int queueStep: 0
    property bool inQueueMode: false
    property bool pendingOpIsCheck: false
    property string currentOpLabel: ""
    // Which update type is actively running: "packages", "flatpak", or a specific app_id
    property string activeUpdateType: ""
    // The pkg object for the currently running per-item update (used to prune cache on success)
    property var currentPkg: null

    // Tracks successfully completed updates so rows can disappear.
    // Keys: specific app_id, "__packages__" for all rpm rows, "__flatpak__" for flatpak rows.
    property var completedUpdates: ({})


    // Called by main.qml's 500ms timer when the daemon writes a check-trigger.
    // Reloads the full updates page from the daemon cache — handles both new
    // update results and reboot_required in one pass.
    function loadFromDaemonCache() {
        if (!checking && !updating) {
            _loadFromCache();
        }
    }

    // On activate: show daemon/tray cached update data only.
    // Fresh checks are started by the daemon/tray trigger or the manual Check button.
    function activate() {
        if (checking || updating) return;

        var cached = backend.loadUpdatesCache();
        if (cached && cached.length > 0) {
            try {
                updateData = JSON.parse(cached);
                rebootRequired = updateData.reboot_required === true;
                backend.setPendingUpdateCount(totalUpdates);
                daemonPollTimer.stop();
                checking = false;
            } catch(e) { updateData = {}; }
        } else if (updateData === null) {
            // No cache yet: wait for the daemon/tray check to publish one.
            checking = true;
            daemonPollTimer.start();
        }
    }

    function _loadFromCache() {
        var cached = backend.loadUpdatesCache();
        if (cached && cached.length > 0) {
            daemonPollTimer.stop();
            checking = false;
            try {
                updateData = JSON.parse(cached);
                rebootRequired = updateData.reboot_required === true;
                // Keep badge in sync
                backend.setPendingUpdateCount(totalUpdates);
            } catch(e) { updateData = {}; }
        } else {
            // Daemon hasn't written cache yet — wait for it
            checking = true;
            updateData = null;
            daemonPollTimer.start();
        }
    }

    // Manual refresh — runs a full check via the backend op system
    function checkUpdates() {
        daemonPollTimer.stop();
        checking = true;
        updating = false;
        updateData = null;
        rebootRequired = false;
        pendingOpIsCheck = true;
        inQueueMode = false;
        completedUpdates = ({});
        backend.checkUpdates();
        pollTimer.start();
    }

    // Poll the op while a manual check (or update) is in progress
    Timer {
        id: pollTimer
        interval: 400
        repeat: true
        onTriggered: {
            backend.pollOp();

            if (!backend.opRunning) {
                pollTimer.stop();
                if (inQueueMode) {
                    var finishedStep = updateQueue[queueStep];
                    // Record batch completion for row hiding.
                    // IMPORTANT: use Object.assign to create a NEW object so QML detects the change.
                    if (backend.opResult === 1) {
                        var co = Object.assign({}, completedUpdates);
                        if (finishedStep === "packages") co["__packages__"] = true;
                        else if (finishedStep === "flatpak") co["__flatpak__"] = true;
                        else if (finishedStep === "appimages") {
                            var ap = updatesPage.currentPkg;
                            if (ap) co[ap.id || ap.app_id || ap.name || ""] = true;
                            updatesPage.currentPkg = null;
                        }
                        completedUpdates = co;
                    }
                    if (finishedStep === "appimages") {
                        // Loop until every AppImage in the list is updated.
                        var stillPending = updateData.appimages.some(function(p) {
                            var id = p.id || p.app_id || p.name || "";
                            return !completedUpdates[id];
                        });
                        if (stillPending && backend.opResult === 1) {
                            _runQueueStep();
                            return;
                        }
                    }
                    queueStep++;
                    if (queueStep < updateQueue.length) {
                        _runQueueStep();
                    } else {
                        // Entire queue done
                        inQueueMode = false;
                        updateQueue = [];
                        updating = false;
                        currentOpLabel = "";
                        activeUpdateType = "";
                        backend.clearUpdatesCache();
                        // Don't reload cache — rows already hidden via completedUpdates
                    }
                } else {
                    var savedType = activeUpdateType;
                    checking = false;
                    updating = false;
                    currentOpLabel = "";
                    activeUpdateType = "";
                    if (pendingOpIsCheck) {
                        if (backend.opResult === 1) {
                            try { updateData = JSON.parse(backend.readLog()); }
                            catch(e) { updateData = {}; }
                            rebootRequired = updateData && updateData.reboot_required === true;
                            backend.setPendingUpdateCount(totalUpdates);
                        }
                    } else if (backend.opResult === 1) {
                        // Individual or section update succeeded — record for row hiding.
                        // IMPORTANT: use Object.assign to create a NEW object so QML detects the change.
                        var co2 = Object.assign({}, completedUpdates);
                        if (savedType === "packages") {
                            co2["__packages__"] = true;
                            backend.clearUpdatesCache();
                        } else if (savedType === "flatpak") {
                            co2["__flatpak__"] = true;
                            backend.clearUpdatesCache();
                        } else if (savedType) {
                            // Per-item update: prune just this entry from the cache
                            var p = updatesPage.currentPkg;
                            if (p) {
                                if (p.pkg_type === "flatpak") {
                                    backend.pruneCacheEntry("flatpak", "app_id", p.app_id || p.id || "");
                                } else if (p.pkg_type === "appimage") {
                                    backend.pruneCacheEntry("appimages", "id", p.id || p.app_id || "");
                                } else {
                                    backend.pruneCacheEntry("packages", "name", p.name || "");
                                }
                            }
                            co2[savedType] = true;
                            updatesPage.currentPkg = null;
                        }
                        completedUpdates = co2;
                    }
                }
            }
        }
    }

    // Poll for the daemon cache file when the daemon is doing its first check
    Timer {
        id: daemonPollTimer
        interval: 3000
        repeat: true
        onTriggered: _loadFromCache()
    }


    // Total update count — subtracts rows already completed so "up to date"
    // state appears automatically as the last row finishes.
    property int totalUpdates: {
        if (!updateData) return 0;
        var cu = completedUpdates;
        var n = 0;
        if (updateData.packages) {
            if (!cu["__packages__"]) {
                updateData.packages.forEach(function(p) {
                    var id = p.app_id || p.id || p.name || "";
                    if (!cu[id]) n++;
                });
            }
        }
        if (updateData.flatpak) {
            if (!cu["__flatpak__"]) {
                updateData.flatpak.forEach(function(p) {
                    var id = p.app_id || p.id || p.name || "";
                    if (!cu[id]) n++;
                });
            }
        }
        if (updateData.appimages) {
            updateData.appimages.forEach(function(p) {
                var id = p.id || p.name || "";
                if (!cu[id]) n++;
            });
        }
        return n;
    }

    onTotalUpdatesChanged: {
        if (updateData !== null && !checking) {
            backend.setPendingUpdateCount(totalUpdates);
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Top action bar ───────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: 56
            color: palette.button

            RowLayout {
                anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
                spacing: 12

                Label {
                    text: {
                        if (updating) return currentOpLabel || "Updating…";
                        if (checking) return "Checking for updates…";
                        if (updateData === null) return "No update data yet";
                        if (rebootRequired)      return "Reboot required to apply updates";
                        if (totalUpdates === 0)  return "Your system is up to date";
                        return totalUpdates + " update" + (totalUpdates !== 1 ? "s" : "") + " available";
                    }
                    font.pixelSize: 14
                    font.bold: true
                    Layout.fillWidth: true
                }

                BusyIndicator {
                    running: checking || updating
                    visible: checking || updating
                    implicitWidth: 24; implicitHeight: 24
                }

                Button {
                    text: "↻  Check for Updates"
                    visible: !checking && !updating
                    onClicked: checkUpdates()
                }

                Button {
                    text: "⬆  Update All"
                    visible: !checking && !updating && totalUpdates > 0 && !rebootRequired
                    highlighted: true
                    onClicked: _doUpdateAll()
                }

                Button {
                    visible: rebootRequired
                    highlighted: true
                    implicitWidth: 130; implicitHeight: 32
                    background: Rectangle { color: "#1976d2"; radius: 4 }
                    contentItem: Label { text: "🔄 Reboot Now"; color: "white"; font.pixelSize: 13; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    onClicked: backend.rebootSystem()
                }
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

        // ── Content area ─────────────────────────────────────────────────────
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            // Daemon is running its first check
            Column {
                anchors.centerIn: parent
                spacing: 16
                visible: updateData === null && checking

                BusyIndicator { anchors.horizontalCenter: parent.horizontalCenter; running: true }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: "Checking for updates…"
                    color: root.dimText
                    font.pixelSize: 14
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: "The background service is running its check"
                    color: root.dimText
                    font.pixelSize: 12
                }
            }

            // Up to date
            Column {
                anchors.centerIn: parent
                spacing: 16
                visible: updateData !== null && totalUpdates === 0 && !rebootRequired

                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: "✓"
                    font.pixelSize: 64
                    color: "#4caf50"
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: "Your system is up to date"
                    font.pixelSize: 16
                    color: root.dimText
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: "Your system and all apps are up to date."
                    font.pixelSize: 12
                    color: root.dimText
                }
            }

            ScrollView {
                anchors.fill: parent
                contentWidth: availableWidth
                visible: updateData !== null && (totalUpdates > 0 || rebootRequired)
                clip: true

                Column {
                    width: parent.width
                    topPadding: 12
                    bottomPadding: 24
                    leftPadding: 24
                    rightPadding: 24
                    spacing: 16

                    // ── Applications section (GUI packages + flatpak + appimages) ─
                    UpdateSection {
                        title: "Applications"
                        packages: {
                            if (!updateData) return [];
                            var result = [];
                            if (updateData.packages) {
                                updateData.packages.forEach(function(p) {
                                    if (p.gui) result.push(Object.assign({}, p, {pkg_type: "rpm"}));
                                });
                            }
                            if (updateData.flatpak) {
                                updateData.flatpak.forEach(function(p) {
                                    if (!p.runtime) result.push(Object.assign({}, p, {pkg_type: "flatpak"}));
                                });
                            }
                            if (updateData.appimages) {
                                updateData.appimages.forEach(function(p) {
                                    result.push(Object.assign({}, p, {pkg_type: "appimage"}));
                                });
                            }
                            return result;
                        }
                        onUpdateAllClicked: _doSectionUpdate(packages)
                    }

                    // ── Flatpak runtimes / add-ons ────────────────────────────
                    UpdateSection {
                        title: "Runtimes/Add-ons"
                        packages: {
                            if (!updateData || !updateData.flatpak) return [];
                            return updateData.flatpak.filter(function(p) { return p.runtime; })
                                .map(function(p) { return Object.assign({}, p, {pkg_type: "flatpak"}); });
                        }
                        onUpdateAllClicked: _doSectionUpdate(packages)
                    }

                    // ── System packages (non-GUI) ─────────────────────────────
                    UpdateSection {
                        title: "System"
                        packages: {
                            if (!updateData || !updateData.packages) return [];
                            return updateData.packages.filter(function(p) { return !p.gui; })
                                .map(function(p) { return Object.assign({}, p, {pkg_type: "rpm"}); });
                        }
                        onUpdateAllClicked: _doSectionUpdate(packages)
                    }
                }
            }
        }
    }

    // ── Update helpers ────────────────────────────────────────────────────────

    function _doUpdateAll() {
        if (updating || checking) return;
        updateQueue = [];
        if (updateData && updateData.packages && updateData.packages.length > 0)
            updateQueue.push("packages");
        if (updateData && updateData.flatpak && updateData.flatpak.length > 0)
            updateQueue.push("flatpak");
        if (updateData && updateData.appimages && updateData.appimages.length > 0)
            updateQueue.push("appimages");
        if (updateQueue.length === 0) return;
        updating = true;
        inQueueMode = true;
        pendingOpIsCheck = false;
        queueStep = 0;
        _runQueueStep();
    }

    function _runQueueStep() {
        var step = updateQueue[queueStep];
        var total = updateQueue.length;
        var stepNum = queueStep + 1;
        if (step === "packages") {
            currentOpLabel = "Updating packages… (" + stepNum + "/" + total + ")";
            activeUpdateType = "packages";
            backend.upgradePackages();
        } else if (step === "flatpak") {
            currentOpLabel = "Updating Flatpak apps… (" + stepNum + "/" + total + ")";
            activeUpdateType = "flatpak";
            backend.upgradeFlatpak("__upgrade_all__", "flatpak", "");
        } else if (step === "appimages") {
            var remaining = updateData.appimages.filter(function(p) {
                var id = p.id || p.app_id || p.name || "";
                return !completedUpdates[id];
            });
            if (remaining.length === 0) {
                // All AppImages done — advance to next step.
                queueStep++;
                if (queueStep < updateQueue.length) _runQueueStep();
                else {
                    inQueueMode = false;
                    updateQueue = [];
                    updating = false;
                    currentOpLabel = "";
                    activeUpdateType = "";
                    currentPkg = null;
                    backend.clearUpdatesCache();
                }
                return;
            }
            var item = remaining[0];
            currentOpLabel = "Updating " + (item.name || item.id || item.app_id || "") + "… (" + stepNum + "/" + total + ")";
            activeUpdateType = item.id || item.app_id || item.name || "";
            currentPkg = item;
            backend.updateAppImage(activeUpdateType, item.download_url || "", item.new_version || "");
        }
        pollTimer.start();
    }

    function _doSectionUpdate(pkgs) {
        if (updating || checking) return;
        if (!pkgs || pkgs.length === 0) return;
        inQueueMode = true;
        pendingOpIsCheck = false;
        updating = true;
        var hasFlatpak = pkgs.some(function(p) { return p.pkg_type === "flatpak"; });
        var hasRpm     = pkgs.some(function(p) { return p.pkg_type === "rpm"; });
        var hasAi      = pkgs.some(function(p) { return p.pkg_type === "appimage"; });
        updateQueue = [];
        if (hasAi)      updateQueue.push("appimages");
        if (hasFlatpak) updateQueue.push("flatpak");
        if (hasRpm)     updateQueue.push("packages");
        queueStep = 0;
        _runQueueStep();
    }

    // ── UpdateSection component ───────────────────────────────────────────────

    component UpdateSection: Rectangle {
        id: secRoot
        property string title: ""
        property var packages: []
        signal updateAllClicked(var pkgs)

        // Live count: how many rows in this section haven't completed yet.
        // Re-evaluates whenever completedUpdates changes (Object.assign ensures new reference).
        readonly property int remainingCount: {
            var cu = updatesPage.completedUpdates;
            var n = 0;
            for (var i = 0; i < secRoot.packages.length; i++) {
                var p = secRoot.packages[i];
                var id = p.app_id || p.id || p.name || "";
                var done = (id && cu[id]) ||
                           (p.pkg_type === "rpm"     && cu["__packages__"]) ||
                           (p.pkg_type === "flatpak" && cu["__flatpak__"]);
                if (!done) n++;
            }
            return n;
        }

        // Section hides itself when all its rows are done or when there are no packages.
        visible: remainingCount > 0

        width: parent ? parent.width - 48 : 400
        height: secCol.implicitHeight + 24
        radius: 8
        color: root.cardColor
        border.color: palette.mid
        border.width: 1

        Column {
            id: secCol
            anchors { fill: parent; margins: 12 }
            spacing: 0

            RowLayout {
                width: parent.width

                Label {
                    text: secRoot.title
                    font.pixelSize: 14
                    font.bold: true
                    Layout.fillWidth: true
                }

                Label {
                    text: secRoot.remainingCount + " update" + (secRoot.remainingCount !== 1 ? "s" : "")
                    color: root.dimText
                    font.pixelSize: 12
                }

                Item { width: 12 }

                Button {
                    text: "Update All"
                    onClicked: secRoot.updateAllClicked(secRoot.packages)
                }
            }

            Item { width: parent.width; height: 8 }
            Rectangle { width: parent.width; height: 1; color: palette.mid; opacity: 0.2 }

            Repeater {
                model: secRoot.packages

                Column {
                    width: secCol.width
                    spacing: 0

                    // Is this specific row's update currently running?
                    readonly property string _pkgId: modelData.app_id || modelData.id || modelData.name || ""
                    readonly property bool rowUpdating: {
                        if (!updatesPage.updating) return false;
                        var t = updatesPage.activeUpdateType;
                        if (!t) return false;
                        // All RPM rows light up when upgrading packages batch
                        if (t === "packages" && modelData.pkg_type === "rpm") return true;
                        // All flatpak rows light up when upgrading flatpak batch
                        if (t === "flatpak" && modelData.pkg_type === "flatpak") return true;
                        // Legacy type match
                        if (t === modelData.pkg_type) return true;
                        // Individual update: matched by id
                        if (_pkgId && t === _pkgId) return true;
                        return false;
                    }
                    // Has this row's update completed successfully?
                    readonly property bool rowCompleted: {
                        var cu = updatesPage.completedUpdates;
                        if (!cu) return false;
                        if (_pkgId && cu[_pkgId]) return true;
                        if (modelData.pkg_type === "rpm" && cu["__packages__"]) return true;
                        if (modelData.pkg_type === "flatpak" && cu["__flatpak__"]) return true;
                        return false;
                    }
                    property real visualProgress: 0.02
                    readonly property real rowProgress: rowUpdating
                        ? Math.max(visualProgress, backend.opProgress / 100.0)
                        : 0

                    onRowUpdatingChanged: {
                        if (rowUpdating) {
                            visualProgress = 0.02;
                            progressFallbackTimer.restart();
                        } else {
                            progressFallbackTimer.stop();
                        }
                    }

                    Timer {
                        id: progressFallbackTimer
                        interval: 100
                        repeat: true
                        running: rowUpdating
                        onTriggered: {
                            if (!rowUpdating) {
                                stop();
                                return;
                            }
                            if (backend.opProgress > 2) {
                                visualProgress = Math.max(visualProgress, backend.opProgress / 100.0);
                            } else if (visualProgress < 0.95) {
                                visualProgress = Math.min(0.95, visualProgress + 0.01);
                            }
                        }
                    }

                    visible: !rowCompleted

                    Rectangle {
                        width: parent.width
                        height: pkgCol.implicitHeight + 16
                        color: "transparent"

                        Column {
                            id: pkgCol
                            anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: 4; rightMargin: 4 }
                            spacing: 4

                            RowLayout {
                                width: parent.width
                                spacing: 10

                                AppIcon {
                                    iconPath: modelData.icon_path || ""
                                    iconUrl: modelData.icon_url || ""
                                    iconName: modelData.display_name || modelData.name || modelData.app_id || modelData.id || "?"
                                    size: 32
                                }

                                Column {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Label {
                                        text: modelData.display_name || modelData.name || modelData.id || ""
                                        font.pixelSize: 13
                                        font.bold: true
                                        elide: Text.ElideRight
                                        width: parent.width
                                    }

                                    RowLayout {
                                        spacing: 6
                                        Label {
                                            text: {
                                                var cur = modelData.current_version || modelData.version || "";
                                                var nw = modelData.new_version || modelData.version || "";
                                                if (cur && nw && cur !== nw) return cur + "  →  " + nw;
                                                if (nw) return "→  " + nw;
                                                return "";
                                            }
                                            font.pixelSize: 11
                                            color: root.dimText
                                            visible: text !== ""
                                        }
                                        Rectangle {
                                            visible: modelData.pkg_type === "flatpak"
                                            radius: 3
                                            color: modelData.needs_install ? "#1b5e20" : "#1a237e"
                                            width: flatpakLbl.implicitWidth + 8
                                            height: 16
                                            Label {
                                                id: flatpakLbl
                                                anchors.centerIn: parent
                                                text: modelData.needs_install ? "New Install" : "Flatpak"
                                                font.pixelSize: 9
                                                color: "white"
                                            }
                                        }
                                        Rectangle {
                                            visible: modelData.pkg_type === "appimage"
                                            radius: 3
                                            color: "#e65100"
                                            width: aiLbl.implicitWidth + 8
                                            height: 16
                                            Label {
                                                id: aiLbl
                                                anchors.centerIn: parent
                                                text: "AppImage"
                                                font.pixelSize: 9
                                                color: "white"
                                            }
                                        }
                                    }
                                }

                                Button {
                                    text: (modelData.needs_install && modelData.pkg_type === "flatpak") ? "Install" : "Update"
                                    flat: true
                                    visible: !rowUpdating
                                    onClicked: {
                                        var pkg = modelData;
                                        var pkgId = pkg.app_id || pkg.id || pkg.name || "";
                                        updatesPage.inQueueMode = false;
                                        updatesPage.pendingOpIsCheck = false;
                                        updatesPage.updating = true;
                                        updatesPage.currentPkg = pkg;
                                        if (pkg.pkg_type === "flatpak") {
                                            updatesPage.activeUpdateType = pkgId;
                                            if (pkg.needs_install) {
                                                // New runtime branch: pass app_id//branch so flatpak installs the right ref
                                                var installRef = (pkg.app_id || "") + "//" + (pkg.version || "");
                                                backend.upgradeFlatpak(installRef, "flatpak", "");
                                            } else if (pkg.runtime) {
                                                // Runtime patch update: pass app_id//branch so flatpak updates the right ref
                                                var runtimeRef = (pkg.app_id || "") + "//" + (pkg.version || "");
                                                backend.upgradeFlatpak(runtimeRef, "flatpak-update", "");
                                            } else {
                                                backend.upgradeFlatpak(pkg.app_id || pkg.id || "", "flatpak-update", "");
                                            }
                                        } else if (pkg.pkg_type === "appimage") {
                                            updatesPage.activeUpdateType = pkgId;
                                            backend.updateAppImage(pkgId, pkg.download_url || "", pkg.new_version || "");
                                        } else {
                                            // Individual RPM update — only upgrade this specific package
                                            updatesPage.activeUpdateType = pkgId;
                                            backend.upgradePackage(pkg.name);
                                        }
                                        pollTimer.start();
                                    }
                                }
                            }

                            // Per-row throbber — shown while this row's update is running.
                            // Indeterminate bar + text label so the user always sees activity.
                            Column {
                                width: parent.width
                                spacing: 3
                                visible: rowUpdating
                                topPadding: 2
                                bottomPadding: 4

                                Label {
                                    text: "Updating " + (modelData.display_name || modelData.name || modelData.app_id || modelData.id || "") + "…"
                                    font.pixelSize: 10
                                    color: root.dimText
                                    leftPadding: 2
                                }

                                Rectangle {
                                    width: parent.width
                                    height: 8
                                    radius: 4
                                    color: palette.mid
                                    opacity: 0.45
                                    clip: true

                                    Rectangle {
                                        width: parent.width * Math.max(0.04, Math.min(1.0, rowProgress))
                                        height: parent.height
                                        radius: parent.radius
                                        color: palette.highlight
                                    }
                                }
                            }
                        }
                    }

                    Rectangle {
                        width: parent.width
                        height: 1
                        color: palette.mid
                        opacity: 0.12
                        visible: index < secRoot.packages.length - 1
                    }
                }
            }
        }
    }

}

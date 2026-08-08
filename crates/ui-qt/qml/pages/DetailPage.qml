import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: detailPage

    property var app: null
    signal backRequested()

    // The source the user clicked in search/list (e.g. "flatpak", "native",
    // "appimage"). Kept so the detail page stays on that source instead of
    // making the user pick Fedora/Flathub again.
    property string preferredSource: ""

    // Where the detail page was opened from: "dnf", "flatpak" or "category".
    // - dnf: only the native (RPM) source is offered — install goes to dnf directly.
    // - flatpak: only flatpak sources are offered (System vs User scope).
    // - category / other: full source picker.
    property string context: "category"

    // Trim the source list to the set the current browsing context cares about.
    function _filterSourcesByContext(data) {
        if (!data || !Array.isArray(data.sources)) return data;
        var list = data.sources;
        if (detailPage.context === "dnf") {
            list = list.filter(function(s) { return s.source !== "flatpak"; });
        } else if (detailPage.context === "flatpak") {
            list = list.filter(function(s) { return s.source === "flatpak"; });
        }
        if (list.length === 0) list = data.sources;
        var copy = {};
        for (var k in data) copy[k] = data[k];
        copy.sources = list;
        return copy;
    }

    function _selectBestSource(appData) {
        var bestIdx = 0;
        if (Array.isArray(appData.sources) && appData.sources.length > 1) {
            // 1) Honor the source the user clicked on
            if (detailPage.preferredSource !== "") {
                for (var p = 0; p < appData.sources.length; p++) {
                    if (appData.sources[p].source === detailPage.preferredSource) {
                        bestIdx = p;
                        return bestIdx;
                    }
                }
            }
            // 2) Default: native (index 0); pick the installed source if native
            //    is not installed but another one is.
            if (!appData.sources[0].installed) {
                for (var i = 1; i < appData.sources.length; i++) {
                    if (appData.sources[i].installed) {
                        bestIdx = i;
                        break;
                    }
                }
            }
        }
        return bestIdx;
    }

    function loadApp(appData) {
        app = _filterSourcesByContext(appData);
        screenshotIndex = 0;
        preferredSource = appData.source || "";
        sourceSelector.currentIndex = _selectBestSource(app);
        _reloadScreenshots();
        _loadAddons();

        // Fetch full record for multi-source data
        if (app && app.id) {
            backend.loadAppById(app.id);
            detailFetchTimer.start();
        }
    }

    // Reload screenshotModel from the currently selected source (or primary app).
    function _reloadScreenshots() {
        screenshotIndex = 0;
        screenshotModel.clear();
        var src = selectedSource();
        var shots = [];
        if (src && src !== app && Array.isArray(src.screenshots) && src.screenshots.length > 0) {
            shots = src.screenshots;
        } else if (app && app.screenshots) {
            shots = app.screenshots;
        }
        for (var i = 0; i < Math.min(shots.length, 8); i++) {
            screenshotModel.append({ url: shots[i] });
        }
    }

    // Computed display object: merges selected source's content fields over the
    // primary app so all labels update reactively when the source selector changes.
    property var displayApp: {
        var _idx = sourceSelector.currentIndex; // reactive dependency
        if (!app) return null;
        if (!Array.isArray(app.sources) || app.sources.length <= 1) return app;
        var src = app.sources[_idx] || app.sources[0];
        if (!src) return app;
        return {
            name:         app.name,
            id:           src.id          || app.id,
            icon_path:    src.icon_path   || app.icon_path,
            icon_url:     src.icon_url    || app.icon_url,
            summary:      src.summary     || app.summary,
            description:  src.description || app.description,
            developer:    src.developer   || app.developer,
            version:      src.version     || app.version,
            license:      src.license     || app.license,
            url_homepage: src.url_homepage|| app.url_homepage,
            url_bugtracker: src.url_bugtracker || app.url_bugtracker,
            url_donation:   src.url_donation   || app.url_donation,
            url_help:       src.url_help       || app.url_help,
            url_faq:        src.url_faq        || app.url_faq,
            url_vcs_browser: src.url_vcs_browser || app.url_vcs_browser,
            url_contribute: src.url_contribute || app.url_contribute,
            package_name: src.package_name|| app.package_name,
            source:       src.source      || app.source,
            installed:    src.installed,
            remote:       src.remote !== undefined ? src.remote : app.remote,
            user_remote:  src.user_remote === true,
            sources:      app.sources,
        };
    }

    property var headerLinks: _buildHeaderLinks()

    function _plainDescriptionText(text) {
        var value = (text || "").toString();
        value = value.replace(/<br\s*\/?>/gi, "\n");
        value = value.replace(/<\/p>/gi, "\n\n");
        value = value.replace(/<[^>]*>/g, "");
        value = value.replace(/&nbsp;/g, " ");
        value = value.replace(/&amp;/g, "&");
        value = value.replace(/&lt;/g, "<");
        value = value.replace(/&gt;/g, ">");
        value = value.replace(/&quot;/g, "\"");
        value = value.replace(/&#39;/g, "'");
        return value;
    }

    function _formattedDescription() {
        if (!displayApp) return "";
        return _plainDescriptionText(displayApp.description || "");
    }

    function _bestLink(field) {
        var candidates = [];
        if (displayApp) candidates.push(displayApp);
        if (app) candidates.push(app);
        if (app && Array.isArray(app.sources)) {
            for (var i = 0; i < app.sources.length; i++) {
                candidates.push(app.sources[i]);
            }
        }

        for (var j = 0; j < candidates.length; j++) {
            var value = candidates[j] ? (candidates[j][field] || "") : "";
            if (value.length > 0) return value;
        }
        return "";
    }

    function _buildHeaderLinks() {
        var links = [];
        var seen = {};

        function add(label, field) {
            var url = _bestLink(field);
            if (!url || seen[url]) return;
            seen[url] = true;
            links.push({ label: label, url: url });
        }

        add("Website", "url_homepage");
        add("Donate", "url_donation");
        add("Issues", "url_bugtracker");
        add("Help", "url_help");
        add("FAQ", "url_faq");
        add("Source", "url_vcs_browser");
        add("Contribute", "url_contribute");

        return links;
    }

    // Async full-detail fetch — polls backend.detailReady, never touches opRunning.
    Timer {
        id: detailFetchTimer
        interval: 300
        repeat: true
        onTriggered: {
            backend.pollDetail();
            if (backend.detailReady) {
                detailFetchTimer.stop();
                var json = backend.readDetail();
                try {
                    var fullApp = JSON.parse(json);
                    if (fullApp && fullApp.id) {
                        detailPage.app = detailPage._filterSourcesByContext(fullApp);
                        detailPage.sourceSelector.currentIndex = detailPage._selectBestSource(detailPage.app);
                        detailPage._reloadScreenshots();
                        detailPage._loadAddons();
                    }
                } catch(e) {}
            }
        }
    }

    // Returns the currently selected source object
    function selectedSource() {
        if (!app) return null;
        if (Array.isArray(app.sources) && app.sources.length > 0) {
            var idx = sourceSelector.currentIndex;
            return app.sources[idx] || app.sources[0];
        }
        return app;
    }

    property int screenshotIndex: 0
    property var addons: []

    property bool installing: false     // main app op in progress
    property string activeAddonId: ""   // addon id currently being installed/removed
    property string pendingMainAction: ""
    property bool overlayLocked: false  // upgrade lock is held

    function _isRepoSource(src) {
        return src !== "flatpak" && src !== "appimage";
    }

    // Poll the upgrade lock every 2 s — disables the install button for repo packages
    // while a dnf5 upgrade operation is running.
    Timer {
        id: overlayLockTimer
        interval: 2000
        repeat: true
        running: detailPage.visible
        onTriggered: {
            var src = detailPage.displayApp ? (detailPage.displayApp.source || "") : "";
            if (detailPage._isRepoSource(src)) {
                detailPage.overlayLocked = backend.isOverlayUpdateRunning();
            } else {
                detailPage.overlayLocked = false;
            }
        }
    }

    function _setCurrentInstallState(installed) {
        if (!app) return;
        var copy = {};
        for (var key in app)
            copy[key] = app[key];
        copy.installed = installed;

        if (Array.isArray(app.sources)) {
            copy.sources = [];
            for (var i = 0; i < app.sources.length; i++) {
                var src = {};
                for (var srcKey in app.sources[i])
                    src[srcKey] = app.sources[i][srcKey];
                if (i === sourceSelector.currentIndex)
                    src.installed = installed;
                copy.sources.push(src);
            }
        }

        app = copy;
    }

    // ── Install / Remove operation timer ──────────────────────────────────────
    // Called by the global installPollTimer in main.qml when the op finishes.
    function onQueueOpFinished() {
        if (detailPage.activeAddonId !== "") {
            detailPage.activeAddonId = "";
            detailPage._loadAddons();
        } else {
            detailPage.installing = false;
            detailPage.pendingMainAction = "";
            if (detailPage.app && detailPage.app.id) {
                backend.loadAppById(detailPage.app.id);
                detailFetchTimer.start();
            }
        }
    }

    function _loadAddons() {
        var da = displayApp;
        var id  = da ? (da.id  || "") : "";
        var src = da ? (da.source || "") : "";
        if (!id || !src) { addons = []; return; }
        backend.loadAddons(id, src);
        addonsPollTimer.start();
    }

    Timer {
        id: addonsPollTimer
        interval: 200
        repeat: true
        onTriggered: {
            backend.pollAddons();
            if (backend.addonsReady) {
                addonsPollTimer.stop();
                try {
                    addons = JSON.parse(backend.readAddons()) || [];
                } catch(e) { addons = []; }
            }
        }
    }

    // Called when navigating away — stops timers and releases heavy data.
    function clear() {
        detailFetchTimer.stop();
        addonsPollTimer.stop();
        overlayLockTimer.stop();
        overlayLocked = false;
        app        = null;
        addons     = [];
        screenshotModel.clear();
    }

    ListModel { id: screenshotModel }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Top bar ───────────────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: 48
            color: palette.button

            RowLayout {
                anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
                spacing: 8

                Button {
                    text: "← Back"
                    flat: true
                    onClicked: detailPage.backRequested()
                }

                Item { Layout.fillWidth: true }

                // Add-ons button — shown when add-ons exist and app is not an appimage
                Button {
                    visible: detailPage.addons.length > 0
                             && app != null
                             && (app.source || "") !== "appimage"
                    text: "Add-ons"
                    onClicked: addonsPopup.open()
                }

                // Overlay-update lock warning — repo packages only
                Label {
                    visible: detailPage.overlayLocked
                    text: "System update in progress…"
                    font.pixelSize: 11
                    color: palette.text
                    opacity: 0.7
                }

                // Install / Remove button — not shown for appimages (no catalog install)
                InstallButton {
                    id: installBtn
                    visible: app != null && (app.source || "") !== "appimage"
                    installed: displayApp != null && displayApp.installed === true
                    busy:      detailPage.installing
                    progress:  backend.opProgress
                    enabled:   !detailPage.installing && !detailPage.overlayLocked

                    Connections {
                        target: sourceSelector
                        function onCurrentIndexChanged() {
                            detailPage._reloadScreenshots();
                            detailPage._loadAddons();
                        }
                    }

                    onInstallClicked: {
                        if (!displayApp) return;
                        detailPage.installing = true;
                        detailPage.pendingMainAction = "install";
                        backend.installApp(displayApp.id || "", displayApp.source || "", displayApp.remote || "",
                                           displayApp.name || "", displayApp.icon_path || "", displayApp.icon_url || "",
                                           displayApp.user_remote === true);
                    }
                    onRemoveClicked: {
                        if (!displayApp) return;
                        detailPage.installing = true;
                        detailPage.pendingMainAction = "remove";
                        backend.removeApp(displayApp.id || "", displayApp.source || "",
                                          displayApp.name || "", displayApp.icon_path || "", displayApp.icon_url || "");
                    }
                }

                // Source selector — shown when both native and Flatpak are available
                ComboBox {
                    id: sourceSelector
                    visible: app != null && Array.isArray(app.sources) && app.sources.length > 1
                    width: 160
                    model: {
                        if (!app || !Array.isArray(app.sources)) return [];
                        return app.sources.map(function(s) {
                            return s.label + (s.installed ? " ✓" : "");
                        });
                    }
                }

                // Uninstall button — appimages only (red, no progress needed)
                InstallButton {
                    visible: app != null && app.source === "appimage"
                    installed: true
                    busy: false
                    onRemoveClicked: backend.removeApp(app.id || "", "appimage",
                                                        app.name || "", app.icon_path || "", app.icon_url || "")
                }
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

        // ── Scroll content ────────────────────────────────────────────────────
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            clip: true

            Column {
                width: parent.width
                topPadding: 24
                bottomPadding: 24
                spacing: 0

                // ── Hero ──────────────────────────────────────────────────────
                Item {
                    width: parent.width
                    height: heroRow.implicitHeight + 32
                    clip: false

                    RowLayout {
                        id: heroRow
                        anchors { left: parent.left; right: parent.right; top: parent.top; leftMargin: 28; rightMargin: 28; topMargin: 8 }
                        spacing: 20

                        AppIcon {
                            property string _ip: displayApp ? (displayApp.icon_path || "") : ""
                            property bool _ipIsUrl: _ip.startsWith("http://") || _ip.startsWith("https://")
                            iconPath: _ipIsUrl ? "" : _ip
                            iconUrl:  _ipIsUrl ? _ip : (displayApp ? (displayApp.icon_url || "") : "")
                            iconName: app ? (app.name || app.id || "?") : "?"
                            size: 80
                            Layout.alignment: Qt.AlignTop
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.alignment: Qt.AlignTop
                            spacing: 4

                            Label {
                                text: app ? (app.name || app.id || "") : ""
                                font.pixelSize: 22
                                font.bold: true
                                wrapMode: Text.WordWrap
                                Layout.fillWidth: true
                            }

                            Flow {
                                visible: detailPage.headerLinks.length > 0
                                Layout.fillWidth: true
                                spacing: 6

                                Repeater {
                                    model: detailPage.headerLinks

                                    Button {
                                        id: linkButton
                                        text: modelData.label
                                        implicitHeight: 30
                                        leftPadding: 12
                                        rightPadding: 12
                                        onClicked: Qt.openUrlExternally(modelData.url)

                                        contentItem: Label {
                                            text: linkButton.text
                                            font.pixelSize: 12
                                            font.bold: true
                                            color: palette.buttonText
                                            horizontalAlignment: Text.AlignHCenter
                                            verticalAlignment: Text.AlignVCenter
                                            elide: Text.ElideRight
                                        }

                                        background: Rectangle {
                                            radius: 15
                                            color: linkButton.down
                                                   ? Qt.darker(palette.button, 1.16)
                                                   : linkButton.hovered
                                                     ? Qt.lighter(palette.button, 1.12)
                                                     : palette.button
                                            border.color: linkButton.hovered ? palette.highlight : palette.mid
                                            border.width: 1
                                        }
                                    }
                                }
                            }

                            Label {
                                text: displayApp ? (displayApp.summary || "") : ""
                                font.pixelSize: 13
                                color: root.dimText
                                wrapMode: Text.WordWrap
                                Layout.fillWidth: true
                                visible: text !== ""
                            }

                            Label {
                                text: displayApp ? (displayApp.developer || "") : ""
                                font.pixelSize: 11
                                color: root.dimText
                                visible: text !== ""
                            }

                            // Meta row: source badge, version, license
                            RowLayout {
                                spacing: 12
                                Layout.topMargin: 4

                                Rectangle {
                                    visible: displayApp != null && (displayApp.source || "") !== ""
                                    radius: 4
                                    color: displayApp ? root.sourceColor(displayApp.source) : palette.button
                                    width: sourceLbl.implicitWidth + 12
                                    height: sourceLbl.implicitHeight + 6
                                    // Hide when sourceSelector is shown (redundant info)
                                    opacity: sourceSelector.visible ? 0 : 1

                                    Label {
                                        id: sourceLbl
                                        anchors.centerIn: parent
                                        text: displayApp ? root.sourceLabel(displayApp.source) : ""
                                        font.pixelSize: 10
                                        color: "white"
                                    }
                                }

                                Label {
                                    text: displayApp && displayApp.version ? "v" + displayApp.version : ""
                                    font.pixelSize: 11
                                    color: root.dimText
                                    visible: text !== ""
                                }

                                Label {
                                    text: displayApp && displayApp.license ? displayApp.license : ""
                                    font.pixelSize: 11
                                    color: root.dimText
                                    visible: text !== ""
                                }
                            }
                        }
                    }
                }

                // Separator
                Rectangle {
                    width: parent.width - 56
                    height: 1
                    anchors.horizontalCenter: parent.horizontalCenter
                    color: palette.mid
                    opacity: 0.2
                }

                Item { width: 1; height: 20 }

                // ── Screenshot carousel ───────────────────────────────────────
                Item {
                    width: parent.width
                    height: screenshotModel.count > 0 ? 320 : 0
                    visible: screenshotModel.count > 0

                    Image {
                        id: mainShot
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: 28 }
                        height: 280
                        source: screenshotModel.count > 0 ? screenshotModel.get(detailPage.screenshotIndex).url : ""
                        fillMode: Image.PreserveAspectFit
                        smooth: true
                        clip: true
                        asynchronous: true
                        cache: false
                    }

                    // Loading / error overlay
                    Rectangle {
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: 28 }
                        height: 280
                        visible: mainShot.status !== Image.Ready
                        color: Qt.rgba(0.5, 0.5, 0.5, 0.08)
                        radius: 4
                        border.color: palette.mid; border.width: 1

                        BusyIndicator {
                            anchors.centerIn: parent
                            running: mainShot.status === Image.Loading
                            visible: mainShot.status === Image.Loading
                            implicitWidth: 32; implicitHeight: 32
                        }
                        Label {
                            anchors.centerIn: parent
                            text: "Screenshot unavailable"
                            color: root.dimText
                            font.pixelSize: 12
                            visible: mainShot.status === Image.Error || mainShot.status === Image.Null
                        }
                    }

                    Button {
                        anchors { left: parent.left; verticalCenter: mainShot.verticalCenter; leftMargin: 34 }
                        visible: screenshotModel.count > 1
                        enabled: detailPage.screenshotIndex > 0
                        onClicked: detailPage.screenshotIndex--
                        width: 44; height: 44
                        flat: true
                        background: Rectangle {
                            radius: 22
                            color: parent.enabled ? "rgba(0,0,0,0.5)" : "rgba(0,0,0,0.2)"
                        }
                        contentItem: Label {
                            text: "‹"
                            color: "white"
                            font.pixelSize: 22
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                    }

                    Button {
                        anchors { right: parent.right; verticalCenter: mainShot.verticalCenter; rightMargin: 34 }
                        visible: screenshotModel.count > 1
                        enabled: detailPage.screenshotIndex < screenshotModel.count - 1
                        onClicked: detailPage.screenshotIndex++
                        width: 44; height: 44
                        flat: true
                        background: Rectangle {
                            radius: 22
                            color: parent.enabled ? "rgba(0,0,0,0.5)" : "rgba(0,0,0,0.2)"
                        }
                        contentItem: Label {
                            text: "›"
                            color: "white"
                            font.pixelSize: 22
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                    }

                    Row {
                        anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter }
                        spacing: 8
                        Repeater {
                            model: screenshotModel.count
                            Label {
                                text: "●"
                                font.pixelSize: 10
                                color: index === detailPage.screenshotIndex ? palette.highlight : palette.mid
                            }
                        }
                    }
                }

                Item { width: 1; height: 20 }

                // ── Description ───────────────────────────────────────────────
                Column {
                    width: parent.width - 56
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 8
                    visible: displayApp != null && detailPage._formattedDescription() !== ""

                    Label {
                        text: "About this app"
                        font.pixelSize: 15
                        font.bold: true
                    }

                    Label {
                        text: detailPage._formattedDescription()
                        textFormat: Text.PlainText
                        font.pixelSize: 13
                        font.bold: false
                        wrapMode: Text.WordWrap
                        width: parent.width
                        color: palette.text
                        lineHeight: 1.15
                    }
                }

                Item { width: 1; height: 24 }

                // ── Info cards ────────────────────────────────────────────────
                Row {
                    width: parent.width - 56
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 12
                    visible: displayApp != null

                    Repeater {
                        model: {
                            if (!displayApp) return [];
                            var cards = [];
                            if (displayApp.url_homepage) cards.push({ label: "Website",   value: displayApp.url_homepage });
                            if (displayApp.package_name && app && app.source !== "appimage")
                                cards.push({ label: displayApp.source === "flatpak" ? "Flatpak ID" : "Package", value: displayApp.package_name });
                            if (displayApp.developer)    cards.push({ label: "Developer", value: displayApp.developer });
                            if (app && app.source === "appimage" && app.installed_path)
                                cards.push({ label: "Path", value: app.installed_path });
                            return cards;
                        }

                        Rectangle {
                            width: 160
                            height: 60
                            radius: 8
                            color: root.cardColor
                            border.color: palette.mid
                            border.width: 1

                            Column {
                                anchors { fill: parent; margins: 10 }
                                spacing: 4
                                Label { text: modelData.label; font.pixelSize: 10; color: root.dimText }
                                Label { text: modelData.value; font.pixelSize: 11; elide: Text.ElideRight; width: parent.width }
                            }
                        }
                    }
                }

                Item { width: 1; height: 24 }

                // ── AppImage update settings ───────────────────────────────────
                Column {
                    width: parent.width - 56
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 12
                    topPadding: 24
                    visible: app != null && app.source === "appimage"

                    Label {
                        text: "Update Settings"
                        font.pixelSize: 15
                        font.bold: true
                    }

                    Rectangle {
                        width: parent.width
                        height: aiSettingsCol.implicitHeight + 24
                        radius: 8
                        color: root.cardColor
                        border.color: palette.mid
                        border.width: 1

                        Column {
                            id: aiSettingsCol
                            anchors { fill: parent; margins: 12 }
                            spacing: 10

                            // Update source
                            RowLayout {
                                width: parent.width
                                spacing: 12

                                Label {
                                    text: "Source"
                                    font.pixelSize: 12
                                    Layout.preferredWidth: 80
                                }

                                ComboBox {
                                    id: updateSourceCombo
                                    Layout.fillWidth: true
                                    model: ["none", "github", "gitlab", "codeberg", "forgejo", "url"]
                                    Component.onCompleted: {
                                        if (app && app.update_source) {
                                            var idx = model.indexOf(app.update_source);
                                            currentIndex = idx >= 0 ? idx : 0;
                                        }
                                    }

                                    Connections {
                                        target: detailPage
                                        function onAppChanged() {
                                            if (app && app.update_source) {
                                                var idx = updateSourceCombo.model.indexOf(app.update_source);
                                                updateSourceCombo.currentIndex = idx >= 0 ? idx : 0;
                                            }
                                        }
                                    }
                                }
                            }

                            // Update URL (hidden for "none")
                            RowLayout {
                                width: parent.width
                                spacing: 12
                                visible: updateSourceCombo.currentText !== "none"

                                Label {
                                    text: updateSourceCombo.currentText === "github" || updateSourceCombo.currentText === "codeberg" || updateSourceCombo.currentText === "forgejo" ? "owner/repo"
                                        : updateSourceCombo.currentText === "gitlab" ? "namespace/repo"
                                        : "URL"
                                    font.pixelSize: 12
                                    Layout.preferredWidth: 80
                                }

                                TextField {
                                    id: updateUrlField
                                    Layout.fillWidth: true
                                    placeholderText: updateSourceCombo.currentText === "github" ? "e.g. owner/myapp"
                                                   : updateSourceCombo.currentText === "codeberg" ? "e.g. owner/myapp"
                                                   : updateSourceCombo.currentText === "forgejo" ? "e.g. https://git.example.org/owner/myapp"
                                                   : updateSourceCombo.currentText === "gitlab"  ? "e.g. group/myapp"
                                                   : "https://example.com/releases/latest"
                                    text: app ? (app.update_url || "") : ""
                                    font.pixelSize: 12
                                }
                            }

                            // Pattern (hidden for "none")
                            RowLayout {
                                width: parent.width
                                spacing: 12
                                visible: updateSourceCombo.currentText !== "none"

                                Label {
                                    text: "Pattern"
                                    font.pixelSize: 12
                                    Layout.preferredWidth: 80
                                }

                                TextField {
                                    id: updatePatternField
                                    Layout.fillWidth: true
                                    placeholderText: "e.g. *.AppImage"
                                    text: app ? (app.update_pattern || "") : ""
                                    font.pixelSize: 12
                                }
                            }

                            // Pre-releases
                            RowLayout {
                                width: parent.width
                                spacing: 12
                                visible: updateSourceCombo.currentText === "github"
                                         || updateSourceCombo.currentText === "codeberg"
                                         || updateSourceCombo.currentText === "forgejo"

                                CheckBox {
                                    id: prereleaseCheck
                                    text: "Allow pre-releases"
                                    font.pixelSize: 12
                                }
                            }

                            // Save button + status
                            RowLayout {
                                width: parent.width
                                spacing: 12

                                Button {
                                    text: "Save Settings"
                                    highlighted: true
                                    onClicked: {
                                        if (!app) return;
                                        var result = JSON.parse(backend.saveAppImageSettings(
                                            app.id || "",
                                            updateSourceCombo.currentText,
                                            updateUrlField.text.trim(),
                                            updatePatternField.text.trim(),
                                            prereleaseCheck.checked
                                        ));
                                        saveStatusLabel.text = result.ok ? "✓ Saved" : "✗ " + result.msg;
                                        saveStatusLabel.color = result.ok ? "#4caf50" : "#e53935";
                                        saveStatusTimer.restart();
                                    }
                                }

                                Label {
                                    id: saveStatusLabel
                                    text: ""
                                    font.pixelSize: 12
                                }

                                Timer {
                                    id: saveStatusTimer
                                    interval: 3000
                                    onTriggered: saveStatusLabel.text = ""
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Add-ons popup ─────────────────────────────────────────────────────────
    Popup {
        id: addonsPopup
        modal: true
        anchors.centerIn: parent
        width: Math.min(480, detailPage.width - 48)
        height: Math.min(520, detailPage.height - 80)
        padding: 0
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // Header
            Rectangle {
                Layout.fillWidth: true
                height: 48
                color: root.cardColor
                radius: 4

                RowLayout {
                    anchors { fill: parent; leftMargin: 16; rightMargin: 12 }
                    spacing: 8

                    Label {
                        text: "Add-ons"
                        font.pixelSize: 15
                        font.bold: true
                        Layout.fillWidth: true
                    }

                    Button {
                        text: "✕"
                        flat: true
                        implicitWidth: 32; implicitHeight: 32
                        onClicked: addonsPopup.close()
                    }
                }
            }

            Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

            // Scrollable add-on list
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                contentWidth: availableWidth
                clip: true

                Column {
                    width: parent.width
                    topPadding: 8
                    bottomPadding: 8
                    spacing: 0

                    Repeater {
                        model: detailPage.addons

                        Column {
                            width: parent.width
                            spacing: 0

                            Rectangle {
                                width: parent.width
                                height: addonRow.implicitHeight + 20
                                color: "transparent"

                                RowLayout {
                                    id: addonRow
                                    anchors {
                                        left: parent.left; right: parent.right
                                        verticalCenter: parent.verticalCenter
                                        leftMargin: 16; rightMargin: 16
                                    }
                                    spacing: 12

                                    Column {
                                        Layout.fillWidth: true
                                        spacing: 3

                                        Label {
                                            text: modelData.name || modelData.id || ""
                                            font.pixelSize: 13
                                            font.bold: true
                                            elide: Text.ElideRight
                                            width: parent.width
                                        }
                                        Label {
                                            text: modelData.summary || ""
                                            font.pixelSize: 11
                                            color: root.dimText
                                            elide: Text.ElideRight
                                            width: parent.width
                                            visible: text !== ""
                                        }
                                    }

                                    InstallButton {
                                        installed: modelData.installed === true
                                        busy:      detailPage.activeAddonId === (modelData.id || "")
                                        progress:  backend.opProgress
                                        implicitWidth: 110; implicitHeight: 30

                                        onInstallClicked: {
                                            var id  = modelData.id || "";
                                            var src = modelData.source || "flatpak";
                                            detailPage.activeAddonId = id;
                                            backend.installApp(id, src, modelData.remote || "",
                                                               modelData.name || "", modelData.icon_path || "", modelData.icon_url || "",
                                                               modelData.user_remote === true);
                                        }
                                        onRemoveClicked: {
                                            var id  = modelData.id || "";
                                            var src = modelData.source || "flatpak";
                                            detailPage.activeAddonId = id;
                                            backend.removeApp(id, src,
                                                              modelData.name || "", modelData.icon_path || "", modelData.icon_url || "");
                                        }
                                    }
                                }
                            }

                            Rectangle {
                                width: parent.width
                                height: 1
                                color: palette.mid
                                opacity: 0.12
                                visible: index < detailPage.addons.length - 1
                            }
                        }
                    }

                    // Empty state
                    Label {
                        width: parent.width
                        text: "No add-ons available"
                        color: root.dimText
                        font.pixelSize: 13
                        horizontalAlignment: Text.AlignHCenter
                        topPadding: 24; bottomPadding: 24
                        visible: detailPage.addons.length === 0
                    }
                }
            }
        }
    }

    // Re-initialize AppImage settings fields when app changes
    onAppChanged: {
        if (app && app.source === "appimage") {
            var idx = updateSourceCombo.model.indexOf(app.update_source || "none");
            updateSourceCombo.currentIndex = idx >= 0 ? idx : 0;
            updateUrlField.text = app.update_url || "";
            updatePatternField.text = app.update_pattern || "";
            prereleaseCheck.checked = !!app.allow_prerelease;
        }
    }
}

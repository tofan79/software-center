import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import org.softwarecenter.Software 1.0
import "pages"
import "components"

ApplicationWindow {
    id: root
    visible: false   // shown explicitly in Component.onCompleted so we can center first
    width: 1280
    height: 800
    minimumWidth: 900
    minimumHeight: 600
    title: "Software Center"

    // Close to tray — hide window instead of quitting
    onClosing: function(close) {
        close.accepted = false;
        root.visible = false;
    }

    // Center the window on the primary screen (index 0).
    // Called before making the window visible so the compositor always
    // places it in the middle of the primary display, not at a stale position.
    function centerOnPrimaryScreen() {
        var screens = Qt.application.screens;
        if (!screens || screens.length === 0) return;
        // Qt.application.screens[0] is the primary screen.
        var s = screens[0];
        root.x = s.virtualX + Math.round((s.width  - root.width)  / 2);
        root.y = s.virtualY + Math.round((s.height - root.height) / 2);
    }

    SoftwareBackend { id: backend }

    // True when running in a live ISO session — hides the System page
    property bool isLive: backend.isLiveEnvironment()

    // ── Cache readiness ───────────────────────────────────────────────────────
    property bool cacheReady: false

    Timer {
        id: cacheCheckTimer
        interval: 200
        repeat: true
        running: !root.cacheReady
        onTriggered: {
            if (backend.isCacheReady()) {
                root.cacheReady = true;
            }
        }
    }

    // ── Local file install ────────────────────────────────────────────────────
    function showLocalFile(path) {
        var kind = backend.fileKind(path);
        if (kind === "unknown") return;
        localFilePage.activate(path, kind);
        backend.navigate(10);
    }

    // ── Startup init ──────────────────────────────────────────────────────────
    Component.onCompleted: {
        // Pre-warm AppStream cache so first search/browse is instant
        backend.warmCache();
        // Load any cached update count from the last daemon check for badge display
        backend.loadDaemonUpdateCache();
        // If launched via MIME type with a file argument, open it
        var startupPath = backend.readStartupFilePath();
        if (startupPath !== "") {
            Qt.callLater(function() { root.showLocalFile(startupPath); });
        }
        // Show window unless launched with --tray (autostart hidden mode).
        if (!backend.readStartHidden()) {
            root.centerOnPrimaryScreen();
            root.visible = true;
        }
    }

    // ── Global install/remove queue poll — auto-starts when a queue op is active
    // Driven by queueActiveName so it needs no explicit start call from pages.
    Timer {
        id: globalInstallPollTimer
        interval: 300
        repeat: true
        running: backend.queueActiveName !== ""
        onTriggered: {
            backend.pollOp();
            // When the op finishes, notify the detail page so it can refresh state.
            if (!backend.opRunning && backend.queueActiveName === "") {
                if (detailLoader.item && typeof detailLoader.item.onQueueOpFinished === "function")
                    detailLoader.item.onQueueOpFinished();
            }
        }
    }

    // ── Show-request / quit-request poll (tray interactions) ─────────────────
    Timer {
        id: showRequestTimer
        interval: 500
        repeat: true
        running: true
        onTriggered: {
            if (backend.checkQuitRequest()) {
                Qt.quit();
                return;
            }
            if (backend.checkShowRequest()) {
                // Always re-center on the primary screen before showing so the
                // window never restores a stale off-screen or secondary-monitor position.
                root.centerOnPrimaryScreen();
                root.visible = true;
                root.raise();
                root.requestActivate();
                // If a file was forwarded by a second instance, open it;
                // otherwise navigate home.
                var openPath = backend.readStartupFilePath();
                if (openPath !== "") {
                    Qt.callLater(function() { root.showLocalFile(openPath); });
                } else {
                    if (backend.currentPage === 7) root.hideDetail();
                    else backend.navigate(0);   // return to Home
                }
            }
            // Poll for daemon-written check-trigger. Fires after a scheduled
            // check completes. Reloads the updates page from the daemon cache
            // — handles new updates and reboot state.
            if (backend.checkDaemonTrigger()) {
                updatesPage.loadFromDaemonCache();
            }
        }
    }

    // Dim secondary text that works in both light and dark mode
    property color dimText: Qt.rgba(palette.windowText.r, palette.windowText.g, palette.windowText.b, 0.6)
    // Elevated card surface — lighter in dark themes, slightly darker in light themes
    readonly property bool isDarkTheme: palette.window.hslLightness < 0.5
    property color cardColor: isDarkTheme
        ? Qt.lighter(palette.window, 1.25)
        : Qt.darker(palette.window, 1.08)

    // Human-readable source label — native packages come from the system repos
    function sourceLabel(src) {
        if (src === "flatpak") return "Flatpak";
        return "Fedora";
    }

    // Badge color for a source
    function sourceColor(src) {
        if (src === "flatpak") return "#1565c0";
        return "#37474f";   // native
    }

    // Global detail page navigation
    property var detailApp: null
    property int previousPage: 0

    function showDetail(appData) {
        detailApp = appData;
        previousPage = backend.currentPage;
        if (!detailLoader.active) {
            // First open: activate and let onLoaded call loadApp.
            detailLoader.active = true;
        } else if (detailLoader.item) {
            // Already live: reuse the existing component, just swap the app data.
            detailLoader.item.loadApp(appData);
        }
        backend.navigate(7);
    }

    function hideDetail() {
        detailLoader.active = false;  // destroy component, freeing all memory
        backend.navigate(previousPage);
    }

    // Navigate to explore page with a category (called from sidebar tree)
    function navigateCategory(cat, label, subcats) {
        backend.navigate(1);
        if (subcats && subcats.length > 0) {
            explorePage.loadCategoryWithSubs(cat, label, subcats);
        } else {
            explorePage.showCategory(cat, label);
        }
    }

    // ── Category tree data (matches Python CATEGORY_TREE exactly) ─────────────
    property var categoryTree: [
        { label: "🎮 Games",        cat: "Game",        subs: [
            { label: "Action",         cat: "ActionGame"    },
            { label: "Adventure",      cat: "AdventureGame" },
            { label: "Arcade",         cat: "ArcadeGame"    },
            { label: "Board Games",    cat: "BoardGame"     },
            { label: "Card Games",     cat: "CardGame"      },
            { label: "Emulators",      cat: "Emulator"      },
            { label: "Kids",           cat: "KidsGame"      },
            { label: "Logic",          cat: "LogicGame"     },
            { label: "Role Playing",   cat: "RolePlaying"   },
            { label: "Shooter",        cat: "Shooter"       },
            { label: "Simulation",     cat: "Simulation"    },
            { label: "Sports",         cat: "SportsGame"    },
            { label: "Strategy",       cat: "StrategyGame"  },
        ]},
        { label: "🌐 Internet",      cat: "Network",     subs: [
            { label: "Web Browsers",   cat: "WebBrowser"      },
            { label: "Email",          cat: "Email"           },
            { label: "Chat & IM",      cat: "InstantMessaging" },
            { label: "File Sharing",   cat: "FileTransfer"    },
            { label: "News & RSS",     cat: "News"            },
            { label: "Remote Access",  cat: "RemoteAccess"    },
        ]},
        { label: "🎵 Audio & Video", cat: "AudioVideo",  subs: [
            { label: "Music",          cat: "Music"           },
            { label: "Video",          cat: "Video"           },
            { label: "Editors",        cat: "AudioVideoEditing" },
            { label: "MIDI",           cat: "Midi"            },
            { label: "TV",             cat: "TV"              },
            { label: "Recorders",      cat: "Recorder"        },
            { label: "Mixers",         cat: "Mixer"           },
        ]},
        { label: "📷 Graphics",      cat: "Graphics",    subs: [
            { label: "2D Editors",     cat: "2DGraphics"      },
            { label: "3D Editors",     cat: "3DGraphics"      },
            { label: "Photography",    cat: "Photography"     },
            { label: "Viewers",        cat: "Viewer"          },
            { label: "Publishing",     cat: "Publishing"      },
            { label: "Scanning",       cat: "Scanning"        },
        ]},
        { label: "💼 Productivity",  cat: "Office",      subs: [
            { label: "Office Suite",   cat: "WordProcessor"   },
            { label: "Spreadsheets",   cat: "Spreadsheet"     },
            { label: "Presentation",   cat: "Presentation"    },
            { label: "Calendar",       cat: "Calendar"        },
            { label: "Finance",        cat: "Finance"         },
            { label: "Contacts",       cat: "ContactManagement" },
            { label: "Project Mgmt",   cat: "ProjectManagement" },
        ]},
        { label: "🛠 Development",   cat: "Development", subs: [
            { label: "IDEs",           cat: "IDE"             },
            { label: "Version Control",cat: "RevisionControl" },
            { label: "Debuggers",      cat: "Debugger"        },
            { label: "Web Dev",        cat: "WebDevelopment"  },
            { label: "Database",       cat: "Database"        },
            { label: "Data Viz",       cat: "DataVisualization" },
        ]},
        { label: "⚙️ System",         cat: "System",      subs: [
            { label: "File Managers",  cat: "FileManager"     },
            { label: "Terminal",       cat: "TerminalEmulator" },
            { label: "Monitors",       cat: "Monitor"         },
            { label: "Security",       cat: "Security"        },
            { label: "Settings",       cat: "Settings"        },
            { label: "Package Mgmt",   cat: "PackageManager"  },
        ]},
        { label: "🧰 Utilities",     cat: "Utility",     subs: [
            { label: "Archiving",      cat: "Archiving"       },
            { label: "Calculators",    cat: "Calculator"      },
            { label: "Clocks",         cat: "Clock"           },
            { label: "Text Editors",   cat: "TextEditor"      },
            { label: "File Tools",     cat: "FileTools"       },
            { label: "Dictionary",     cat: "Dictionary"      },
        ]},
        { label: "🔬 Science",       cat: "Science",     subs: [
            { label: "Astronomy",      cat: "Astronomy"       },
            { label: "Chemistry",      cat: "Chemistry"       },
            { label: "Electronics",    cat: "Electronics"     },
            { label: "Engineering",    cat: "Engineering"     },
            { label: "Geography",      cat: "Geography"       },
            { label: "Physics",        cat: "Physics"         },
        ]},
        { label: "🎓 Education",     cat: "Education",   subs: [
            { label: "Languages",      cat: "Languages"       },
            { label: "Mathematics",    cat: "Math"            },
            { label: "Science",        cat: "Science"         },
            { label: "Literature",     cat: "Literature"      },
            { label: "Geography",      cat: "Geography"       },
            { label: "Kids",           cat: "KidsGame"        },
        ]},
    ]

    // Track which top-level category sections are expanded
    property var expandedCats: ({})

    RowLayout {
        anchors { fill: parent; bottomMargin: installBanner.height }
        spacing: 0

        // ── Sidebar ────────────────────────────────────────────────────────────
        Rectangle {
            Layout.preferredWidth: 230
            Layout.fillHeight: true
            color: palette.window
            visible: backend.currentPage !== 7

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                // ── Logo row ────────────────────────────────────────────────────
                Item {
                    Layout.fillWidth: true
                    height: 54

                    RowLayout {
                        anchors { fill: parent; leftMargin: 14; rightMargin: 8 }
                        spacing: 8

                        Image {
                            source: "file:///usr/share/pixmaps/software-center.png"
                            sourceSize.width: 28
                            sourceSize.height: 28
                            Layout.preferredWidth: 28
                            Layout.preferredHeight: 28
                            fillMode: Image.PreserveAspectFit
                            smooth: true
                            mipmap: true
                        }

                        Label {
                            text: "Software Center"
                            font.pixelSize: 13
                            font.bold: true
                            Layout.fillWidth: true
                        }
                    }
                }

                Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid; opacity: 0.3 }

                // ── Nav buttons + category tree ─────────────────────────────────
                ScrollView {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    contentWidth: availableWidth
                    clip: true
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                    Column {
                        width: parent.width
                        topPadding: 8
                        bottomPadding: 12

                        // ── Static nav buttons ──────────────────────────────────
                        Repeater {
                            model: {
                                var items = [
                                    { label: "🏠  Home",       page: 0 },
                                    { label: "📦  Installed",  page: 3 },
                                    { label: "🔄  Updates",    page: 4 },
                                    { label: "⚙️   System",    page: 5 },
                                    { label: "🛠  Settings",   page: 6 },
                                ];
                                return root.isLive
                                    ? items.filter(function(i) { return i.page !== 5; })
                                    : items;
                            }
                            delegate: Rectangle {
                                width: parent.width
                                height: 34
                                color: backend.currentPage === modelData.page
                                    ? Qt.rgba(palette.highlight.r, palette.highlight.g, palette.highlight.b, 0.15)
                                    : navBtnArea.containsMouse ? root.cardColor : "transparent"
                                radius: 4

                                Label {
                                    anchors { left: parent.left; leftMargin: 12; verticalCenter: parent.verticalCenter }
                                    text: modelData.label
                                    font.pixelSize: 13
                                    font.bold: backend.currentPage === modelData.page
                                    color: backend.currentPage === modelData.page
                                        ? palette.highlight : palette.text
                                }

                                MouseArea {
                                    id: navBtnArea
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: backend.navigate(modelData.page)
                                }
                            }
                        }

                        // ── Separator + Categories heading ──────────────────────
                        Item { width: parent.width; height: 10 }

                        Rectangle { width: parent.width - 20; height: 1; color: palette.mid; opacity: 0.3; anchors.horizontalCenter: parent.horizontalCenter }

                        Item { width: parent.width; height: 6 }

                        Label {
                            leftPadding: 14
                            text: "Categories"
                            font.pixelSize: 11
                            font.bold: true
                            color: root.dimText
                        }

                        Item { width: parent.width; height: 4 }

                        // ── Expandable category tree ────────────────────────────
                        Repeater {
                            model: root.categoryTree

                            Column {
                                id: catSection
                                width: parent.width

                                property bool expanded: root.expandedCats[modelData.cat] === true

                                // Top-level row: arrow toggle + category button
                                Rectangle {
                                    width: parent.width
                                    height: 32
                                    color: topCatArea.containsMouse ? root.cardColor : "transparent"
                                    radius: 4

                                    RowLayout {
                                        anchors { fill: parent; leftMargin: 4; rightMargin: 6 }
                                        spacing: 0

                                        // Arrow toggle
                                        Label {
                                            text: catSection.expanded ? "▾" : "▸"
                                            font.pixelSize: 10
                                            color: root.dimText
                                            Layout.preferredWidth: 18
                                            horizontalAlignment: Text.AlignHCenter
                                        }

                                        Label {
                                            text: modelData.label
                                            font.pixelSize: 12
                                            color: backend.currentPage === 1 ? palette.text : palette.text
                                            Layout.fillWidth: true
                                        }
                                    }

                                    MouseArea {
                                        id: topCatArea
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: {
                                            // Toggle expand
                                            var e = root.expandedCats;
                                            e[modelData.cat] = !catSection.expanded;
                                            root.expandedCats = e;
                                            catSection.expanded = e[modelData.cat];
                                            // Navigate to explore with subcats
                                            root.navigateCategory(modelData.cat, modelData.label, modelData.subs);
                                        }
                                    }
                                }

                                // Sub-items (shown when expanded)
                                Column {
                                    width: parent.width
                                    visible: catSection.expanded

                                    Repeater {
                                        model: modelData.subs

                                        Rectangle {
                                            width: parent.width
                                            height: 27
                                            color: subArea.containsMouse ? root.cardColor : "transparent"
                                            radius: 4

                                            Label {
                                                anchors { left: parent.left; leftMargin: 30; verticalCenter: parent.verticalCenter }
                                                text: modelData.label
                                                font.pixelSize: 11
                                                color: palette.text
                                            }

                                            MouseArea {
                                                id: subArea
                                                anchors.fill: parent
                                                hoverEnabled: true
                                                cursorShape: Qt.PointingHandCursor
                                                onClicked: {
                                                    // Navigate to explore with this subcategory
                                                    backend.navigate(1);
                                                    explorePage.showCategory(modelData.cat, modelData.label);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Rectangle {
            width: 1
            Layout.fillHeight: true
            color: palette.mid
            opacity: 0.35
            visible: backend.currentPage !== 7
        }

        // ── Main area ──────────────────────────────────────────────────────────
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // ── Topbar (search) ────────────────────────────────────────────────
            Rectangle {
                Layout.fillWidth: true
                height: 56
                color: palette.button
                visible: backend.currentPage !== 7

                RowLayout {
                    anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
                    spacing: 12

                    // Search field
                    Rectangle {
                        Layout.preferredWidth: 420
                        height: 34
                        radius: 6
                        color: palette.base
                        border.color: searchField.activeFocus ? palette.highlight : palette.mid
                        border.width: searchField.activeFocus ? 2 : 1

                        RowLayout {
                            anchors { fill: parent; leftMargin: 10; rightMargin: 6 }
                            spacing: 6

                            Label {
                                text: "🔍"
                                font.pixelSize: 14
                                color: root.dimText
                            }

                            TextField {
                                id: searchField
                                Layout.fillWidth: true
                                placeholderText: "Search apps…"
                                font.pixelSize: 13
                                background: Rectangle { color: "transparent" }
                                Keys.onReturnPressed: {
                                    var q = text.trim();
                                    if (q.length < 2) return;
                                    backend.navigate(2);
                                    searchPageItem.triggerSearch(q);
                                }
                            }
                        }
                    }

                    Item { Layout.fillWidth: true }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: palette.mid
                opacity: 0.3
                visible: backend.currentPage !== 7
            }

            // ── Page stack ─────────────────────────────────────────────────────
            StackLayout {
                id: pageStack
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: backend.currentPage

                HomePage      { id: homePage }
                ExplorePage   { id: explorePage }
                SearchPage    { id: searchPageItem }
                InstalledPage { id: installedPage }
                UpdatesPage   { id: updatesPage }
                SystemPage    { id: systemPageItem }
                SettingsPage  { id: settingsPage }

                // Page 7: Detail — loaded on demand; active=false destroys it entirely,
                // freeing all widget memory, textures, and JS property bindings.
                Loader {
                    id: detailLoader
                    active: false
                    sourceComponent: Component {
                        DetailPage {
                            onBackRequested: root.hideDetail()
                        }
                    }
                    onLoaded: item.loadApp(root.detailApp)
                }

                // Page 10: Local file install
                LocalFilePage {
                    id: localFilePage
                    onBackRequested: backend.navigate(0)
                }
            }
        }

        // Re-activate data-driven pages when navigated to
        Connections {
            target: backend
            function onCurrentPageChanged() {
                switch (backend.currentPage) {
                    case 3: installedPage.activate(); break;
                    case 4: updatesPage.activate(); break;
                    case 5: systemPageItem.activate(); break;
                    case 7: if (detailLoader.item) detailLoader.item.loadApp(root.detailApp); break;
                }
            }
        }
    }

    // ── Install queue banner — full window width, anchored to bottom ──────────
    Rectangle {
        id: installBanner
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: installBannerVisible ? 72 : 0
        clip: true
        color: palette.button
        z: 10

        property bool installBannerVisible:
            backend.opRunning && backend.queueActiveName !== ""

        Behavior on height { NumberAnimation { duration: 150 } }

        Rectangle {
            width: parent.width
            height: 1
            color: palette.mid
            opacity: 0.35
            anchors.top: parent.top
        }

        ColumnLayout {
            anchors { fill: parent; topMargin: 8; bottomMargin: 8; leftMargin: 14; rightMargin: 14 }
            spacing: 5

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                AppIcon {
                    size: 28
                    iconPath: {
                        var p = backend.queueActiveIconPath;
                        return (p && !p.startsWith("http")) ? p : "";
                    }
                    iconUrl: {
                        var p = backend.queueActiveIconPath;
                        return (p && p.startsWith("http")) ? p : backend.queueActiveIconUrl;
                    }
                    iconName: backend.queueActiveName
                }

                Label {
                    Layout.fillWidth: true
                    text: {
                        var verb = backend.queueIsRemove ? "Removing" : "Installing";
                        return verb + " " + backend.queueActiveName;
                    }
                    font.pixelSize: 13
                    elide: Text.ElideRight
                }

                Label {
                    visible: backend.queueCount > 0
                    text: backend.queueCount + " more in queue"
                    font.pixelSize: 12
                    color: root.dimText
                }
            }

            // Real progress bar driven by backend.opProgress (0–100)
            Rectangle {
                Layout.fillWidth: true
                height: 5
                radius: 2
                color: palette.mid
                opacity: 0.4

                Rectangle {
                    width: Math.max(parent.width * 0.02,
                                    parent.width * Math.min(1.0, backend.opProgress / 100.0))
                    height: parent.height
                    radius: parent.radius
                    color: palette.highlight
                    Behavior on width { NumberAnimation { duration: 80 } }
                }
            }
        }
    }

    // ── Startup loading overlay (direct ApplicationWindow child — no layout conflict) ──
    Rectangle {
        anchors.fill: parent
        color: palette.window
        visible: !root.cacheReady
        z: 100

        Column {
            anchors.centerIn: parent
            spacing: 20

            Image {
                anchors.horizontalCenter: parent.horizontalCenter
                source: "file:///usr/share/pixmaps/software-center.png"
                width: 80; height: 80
                fillMode: Image.PreserveAspectFit
                smooth: true
                mipmap: true
            }

            BusyIndicator {
                anchors.horizontalCenter: parent.horizontalCenter
                running: true
                implicitWidth: 40; implicitHeight: 40
            }

            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Loading software catalog…"
                font.pixelSize: 14
                color: root.dimText
            }
        }
    }
}

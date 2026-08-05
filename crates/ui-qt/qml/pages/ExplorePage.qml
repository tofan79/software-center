import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: explorePage

    // ── State ──────────────────────────────────────────────────────────────────
    property string selectedCategory: ""
    property string selectedLabel: ""
    property var    subcatList: []       // subs when top-level cat was clicked
    property string parentCat: ""
    property string parentLabel: ""
    property var    categoryApps: []
    property bool   loading: false
    property int    visibleCount: 0

    readonly property int pageSize: 60
    property var displayedApps: {
        return categoryApps.slice(0, Math.min(visibleCount, categoryApps.length));
    }

    // ── Public API ─────────────────────────────────────────────────────────────

    function loadCategoryWithSubs(cat, label, subcats) {
        _reset(cat, label);
        subcatList = subcats || [];
        parentCat = "";
        parentLabel = "";
        _fetchApps(cat);
    }

    function showCategory(cat, label) {
        _reset(cat, label);
        subcatList = [];
        _fetchApps(cat);
    }

    function goBack() {
        if (parentCat !== "") {
            var tree = root.categoryTree;
            var subs = [];
            for (var i = 0; i < tree.length; i++) {
                if (tree[i].cat === parentCat) { subs = tree[i].subs; break; }
            }
            loadCategoryWithSubs(parentCat, parentLabel, subs);
        }
    }

    // ── Internal ───────────────────────────────────────────────────────────────

    function _reset(cat, label) {
        pollTimer.stop();
        selectedCategory = cat;
        selectedLabel = label;
        categoryApps = [];
        loading = false;
        visibleCount = 0;
    }

    function _fetchApps(cat) {
        loading = true;
        categoryApps = [];
        backend.loadCategory(cat);
        pollTimer.start();
    }

    function revealMore() {
        if (loading || categoryApps.length === 0)
            return;
        if (visibleCount < categoryApps.length)
            visibleCount = Math.min(categoryApps.length, visibleCount + pageSize);
    }

    function maybeRevealFromScroll() {
        var item = appScroll.contentItem;
        if (!item) return;
        var bottom = item.contentY + appScroll.height;
        if (loading || categoryApps.length === 0) return;
        if (visibleCount >= categoryApps.length) return;
        if (bottom >= item.contentHeight - 320)
            revealMore();
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
                        categoryApps = JSON.parse(backend.readLog());
                        visibleCount = Math.min(pageSize, categoryApps.length);
                    }
                    catch(e) {
                        categoryApps = [];
                    }
                }
            }
        }
    }

    // ── UI ─────────────────────────────────────────────────────────────────────

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Back bar (shown when in a subcategory) ─────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: 44
            color: palette.button
            visible: parentCat !== ""

            RowLayout {
                anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
                spacing: 8

                Button {
                    text: "← " + (parentLabel !== "" ? parentLabel : "Back")
                    flat: true
                    font.pixelSize: 12
                    onClicked: explorePage.goBack()
                }

                Label {
                    text: selectedLabel
                    font.pixelSize: 15
                    font.bold: true
                }

                Item { Layout.fillWidth: true }
            }
        }

        // ── Category heading (top-level, no back bar) ──────────────────────────
        Item {
            Layout.fillWidth: true
            height: 44
            visible: selectedLabel !== "" && parentCat === ""

            Label {
                anchors { left: parent.left; leftMargin: 24; verticalCenter: parent.verticalCenter }
                text: selectedLabel
                font.pixelSize: 18
                font.bold: true
            }
        }

        // ── Scrollable content ─────────────────────────────────────────────────
        ScrollView {
            id: appScroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            clip: true

            Connections {
                target: appScroll.contentItem
                function onContentYChanged() { explorePage.maybeRevealFromScroll(); }
            }

            Column {
                width: parent.width
                topPadding: 8
                bottomPadding: 24
                spacing: 0

                // Subcategories live in the sidebar; the page itself always
                // shows the app grid for the selected category.

                // ── Loading indicator ──────────────────────────────────────────
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

                // ── Empty state ────────────────────────────────────────────────
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    topPadding: 40
                    text: "No apps found in this category."
                    color: root.dimText
                    font.pixelSize: 14
                    visible: !loading && categoryApps.length === 0 && selectedCategory !== ""
                }

                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    topPadding: 60
                    text: "Select a category from the sidebar to browse apps."
                    color: root.dimText
                    font.pixelSize: 14
                    visible: !loading && selectedCategory === ""
                }

                // ── App grid (Flow — wraps to fill width) ──────────────────────
                Flow {
                    width: parent.width - 32
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 12
                    visible: !loading && displayedApps.length > 0
                    topPadding: 4

                    Repeater {
                        model: loading ? [] : explorePage.displayedApps

                        Rectangle {
                            width: 120
                            height: 120
                            radius: 10
                            color: appCardArea.containsMouse ? Qt.lighter(root.cardColor, 1.08) : root.cardColor
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

                            // Installed checkmark badge
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
                                id: appCardArea
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
                    height: categoryApps.length > displayedApps.length ? 64 : 0
                    visible: categoryApps.length > displayedApps.length

                    Row {
                        anchors.centerIn: parent
                        spacing: 12

                        Button {
                            text: "Load More"
                            onClicked: explorePage.revealMore()
                        }

                        Label {
                            text: displayedApps.length + " of " + categoryApps.length + " apps"
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

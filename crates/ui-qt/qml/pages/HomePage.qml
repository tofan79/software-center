import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components"

Item {
    id: homePage

    function activate() {
        if (homeData === null && !loading) {
            loadData();
        }
    }

    property var homeData: null
    property bool loading: false

    function loadData() {
        if (loading) return;
        loading = true;
        homeData = null;
        backend.loadHome();
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
                    var json = backend.homeDataLoaded();
                    try {
                        homeData = JSON.parse(json);
                    } catch(e) {
                        homeData = null;
                    }
                }
            }
        }
    }

    function appsForSection(key) {
        if (!homeData) return [];
        var arr = homeData[key];
        if (!arr) return [];
        return arr;
    }

    ScrollView {
        anchors.fill: parent
        contentWidth: availableWidth
        clip: true

        Column {
            width: parent.width
            spacing: 0
            topPadding: 20
            bottomPadding: 24

            // Loading indicator
            Item {
                width: parent.width
                height: 80
                visible: loading

                Row {
                    anchors.centerIn: parent
                    spacing: 12
                    BusyIndicator { running: loading; implicitWidth: 28; implicitHeight: 28 }
                    Label {
                        text: "Loading home page…"
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }
            }

            // Sections: picks, popular, updated, new
            Repeater {
                model: [
                    { key: "picks",   title: "Editor's Picks"  },
                    { key: "popular", title: "Popular Apps"     },
                    { key: "updated", title: "Recently Updated" },
                    { key: "new",     title: "New Apps"         },
                ]

                Column {
                    width: homePage.width
                    spacing: 0
                    visible: !loading && homeData !== null
                             && appsForSection(modelData.key).length > 0

                    // Section header
                    Item {
                        width: parent.width
                        height: 44

                        Label {
                            anchors {
                                left: parent.left; leftMargin: 24
                                verticalCenter: parent.verticalCenter
                            }
                            text: modelData.title
                            font.pixelSize: 16
                            font.bold: true
                        }
                    }

                    // App grid — wraps to as many rows as needed
                    Flow {
                        id: appFlow
                        width: parent.width - 48
                        anchors.horizontalCenter: parent.horizontalCenter
                        spacing: 12

                        Repeater {
                            model: appsForSection(modelData.key).slice(0, 20)

                            Rectangle {
                                width: 120
                                height: 120
                                radius: 10
                                color: root.cardColor
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

                                MouseArea {
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: root.showDetail(modelData)
                                }
                            }
                        }
                    }

                    Item { width: parent.width; height: 20 }
                }
            }

            // Empty / error state
            Item {
                width: parent.width
                height: 200
                visible: !loading && homeData === null

                Column {
                    anchors.centerIn: parent
                    spacing: 12

                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: "Could not load home page data."
                        color: root.dimText
                    }

                    Button {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: "Retry"
                        onClicked: loadData()
                    }
                }
            }
        }
    }

    Component.onCompleted: {
        activate();
    }
}

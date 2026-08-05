import QtQuick 2.15
import QtQuick.Controls 2.15

// InstallButton — colored action button with in-button progress animation.
// Green for Install, red for Remove.
// When busy: shimmer slide (indeterminate) or left-fill (progress 0-100).

Button {
    id: btn

    // ── Public API ────────────────────────────────────────────────────────────
    property bool installed: false   // true  → show Remove (red)
    property bool busy:      false   // true  → show progress animation, disable
    property real progress:  0       // 0-100; 0 = indeterminate when busy

    signal installClicked()
    signal removeClicked()

    // ── Geometry ──────────────────────────────────────────────────────────────
    implicitWidth:  110
    implicitHeight: 34
    enabled: !busy
    padding: 0

    // ── Visuals ───────────────────────────────────────────────────────────────
    background: Rectangle {
        id: bg
        radius: 4
        // Base color — grey when locked (disabled but not busy), normal otherwise
        color: (!btn.enabled && !btn.busy) ? "#757575" : (btn.installed ? "#c62828" : "#2e7d32")
        clip: true

        // Determinate fill — slides in from the left as progress increases
        Rectangle {
            visible: btn.busy && btn.progress > 0
            y: 0
            height: parent.height
            color: Qt.rgba(1, 1, 1, 0.22)
            width: parent.width * Math.min(btn.progress, 100) / 100
            Behavior on width { NumberAnimation { duration: 150 } }
        }

        // Indeterminate shimmer — white band slides left → right on loop
        Rectangle {
            id: shimmer
            visible: btn.busy && btn.progress <= 0
            y: 0
            height: parent.height
            width: bg.width * 0.38
            color: Qt.rgba(1, 1, 1, 0.20)

            SequentialAnimation on x {
                running: shimmer.visible
                loops:   Animation.Infinite
                NumberAnimation {
                    from:     -shimmer.width
                    to:       bg.width
                    duration: 900
                    easing.type: Easing.InOutSine
                }
            }
        }
    }

    contentItem: Label {
        text: {
            if (btn.busy) return btn.installed ? "Removing…" : "Installing…"
            return btn.installed ? "Remove" : "Install"
        }
        color:                  "white"
        font.pixelSize:         13
        font.bold:              true
        horizontalAlignment:    Text.AlignHCenter
        verticalAlignment:      Text.AlignVCenter
    }

    onClicked: btn.installed ? btn.removeClicked() : btn.installClicked()
}

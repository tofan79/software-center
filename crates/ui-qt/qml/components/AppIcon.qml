import QtQuick 2.15

// AppIcon — shows app icon from icon_path file, icon_url (HTTP), or initials fallback
Item {
    id: root

    property string iconPath: ""
    property string iconUrl: ""    // HTTP/HTTPS URL fallback when no local file
    property string iconName: ""   // fallback initial letter
    property int size: 36

    width: size
    height: size

    // 1. File-based icon (local filesystem)
    Image {
        id: fileIcon
        anchors.fill: parent
        source: root.iconPath ? "file://" + root.iconPath : ""
        visible: root.iconPath !== "" && status === Image.Ready
        fillMode: Image.PreserveAspectFit
        smooth: true
        mipmap: true
    }

    // 2. URL-based icon (fetched over HTTP — QML caches automatically)
    Image {
        id: urlIcon
        anchors.fill: parent
        source: !fileIcon.visible && root.iconUrl ? root.iconUrl : ""
        visible: root.iconUrl !== "" && !fileIcon.visible && status === Image.Ready
        fillMode: Image.PreserveAspectFit
        smooth: true
        mipmap: true
    }

    // 3. Initials fallback
    Rectangle {
        anchors.fill: parent
        radius: root.size * 0.18
        color: palette.button
        visible: !fileIcon.visible && !urlIcon.visible

        Text {
            anchors.centerIn: parent
            text: root.iconName ? root.iconName[0].toUpperCase() : "?"
            font.pixelSize: root.size * 0.44
            font.bold: true
            color: palette.buttonText
        }
    }
}

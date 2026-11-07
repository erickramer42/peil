"""Checks the app's packaged manifest against ADR-0004 (sections 1 and 7, as
amended): the SDK levels, exactly the declared permissions, backups and
device-to-device transfer off, the one activity, and the session service
unexported with the `location` type. Usage: check_manifest.py <AndroidManifest.xml>
"""

import sys
import xml.etree.ElementTree as ET

A = "{http://schemas.android.com/apk/res/android}"

PERMISSIONS = {
    "android.permission.ACCESS_FINE_LOCATION",
    "android.permission.ACCESS_COARSE_LOCATION",
    "android.permission.BLUETOOTH_SCAN",
    "android.permission.NEARBY_WIFI_DEVICES",
    "android.permission.ACCESS_WIFI_STATE",
    "android.permission.CHANGE_WIFI_STATE",
    "android.permission.FOREGROUND_SERVICE",
    "android.permission.FOREGROUND_SERVICE_LOCATION",
    "android.permission.WAKE_LOCK",
    "android.permission.POST_NOTIFICATIONS",
}

NAMESPACE = "io.github.erickramer42.sensio"


def main(path):
    root = ET.parse(path).getroot()
    problems = []

    def expect(what, got, want):
        if got != want:
            problems.append(f"{what}: {got!r}, expected {want!r}")

    expect("package", root.get("package"), "io.github.erickramer42.peil")
    sdk = root.find("uses-sdk")
    expect("minSdkVersion", sdk.get(A + "minSdkVersion") if sdk is not None else None, "31")
    expect("targetSdkVersion", sdk.get(A + "targetSdkVersion") if sdk is not None else None, "36")

    declared = {p.get(A + "name") for p in root.findall("uses-permission")}
    declared |= {p.get(A + "name") for p in root.findall("uses-permission-sdk-23")}
    # The Android Gradle plugin adds this one to every app targeting API 33
    # or later; it keeps the app's own unexported receivers private.
    declared.discard("io.github.erickramer42.peil.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION")
    if not declared:
        problems.append("no permissions found; is this the app's manifest?")
    for p in sorted(declared - PERMISSIONS):
        problems.append(f"permission not in ADR-0004: {p}")
    for p in sorted(PERMISSIONS - declared):
        problems.append(f"permission missing: {p}")
    bt = [p for p in root.findall("uses-permission") if p.get(A + "name") == "android.permission.BLUETOOTH_SCAN"]
    if any(p.get(A + "usesPermissionFlags") for p in bt):
        problems.append("BLUETOOTH_SCAN has usesPermissionFlags (neverForLocation filters beacons out)")

    app = root.find("application")
    if app is None:
        problems.append("no application element")
        app = ET.Element("application")
    expect("allowBackup", app.get(A + "allowBackup"), "false")
    if not app.get(A + "dataExtractionRules"):
        problems.append("no dataExtractionRules")

    activities = app.findall("activity")
    expect("activities", sorted(a.get(A + "name") for a in activities), [f"{NAMESPACE}.MainActivity"])
    services = app.findall("service")
    expect("services", sorted(s.get(A + "name") for s in services), [f"{NAMESPACE}.SessionService"])
    for s in services:
        expect("SessionService exported", s.get(A + "exported"), "false")
        expect("SessionService foregroundServiceType", s.get(A + "foregroundServiceType"), "location")
    for kind in ("receiver", "provider"):
        for e in app.findall(kind):
            problems.append(f"unexpected {kind}: {e.get(A + 'name')}")

    if problems:
        print(f"{path}:", *problems, sep="\n  ")
        return 1
    print(f"{path}: as ADR-0004 says ({len(declared)} permissions, one activity, one service)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))

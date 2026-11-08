package io.github.erickramer42.sensio

/** Permissions by the names Android's settings show them under. */
object Permissions {
    private val GROUPS = mapOf(
        "android.permission.ACCESS_FINE_LOCATION" to "Location",
        "android.permission.ACCESS_COARSE_LOCATION" to "Location",
        "android.permission.BLUETOOTH_SCAN" to "Nearby devices",
        "android.permission.BLUETOOTH_CONNECT" to "Nearby devices",
        "android.permission.NEARBY_WIFI_DEVICES" to "Nearby devices",
        "android.permission.POST_NOTIFICATIONS" to "Notifications",
    )

    /** Each group once, in the order first named; an unknown permission by its own name. */
    fun groups(permissions: List<String>): List<String> =
        permissions.map { GROUPS[it] ?: it.substringAfterLast('.') }.distinct()
}

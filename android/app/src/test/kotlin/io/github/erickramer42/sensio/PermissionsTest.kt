package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Test

class PermissionsTest {
    @Test
    fun permissionsAreNamedAsAndroidsSettingsNameThem() {
        val refused = listOf(
            "android.permission.ACCESS_FINE_LOCATION",
            "android.permission.ACCESS_COARSE_LOCATION",
            "android.permission.BLUETOOTH_SCAN",
            "android.permission.NEARBY_WIFI_DEVICES",
            "android.permission.SOMETHING_NEW",
        )
        assertEquals(listOf("Location", "Nearby devices", "SOMETHING_NEW"), Permissions.groups(refused))
    }
}

package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Test

/** Tracker labels, with the device a format names after its network. */
class LabelsTest {
    private fun ble(tracker: String?, entry: String?) =
        Emitter("ble:aa:aa:aa:00:00:01", "ble", "weak", -90.0, -90.0, 1, "ble", null, null, tracker, null, null, categoryEntry = entry)

    @Test
    fun aFindMyAdvertsDeviceFollowsItsNetwork() {
        assertEquals(
            listOf("Find My AirTag", "Find My accessory", "Find My AirPods", "Find My Apple device", "Find My", "Tile", "SmartThings SmartTag", null),
            listOf(
                ble("apple_find_my", "apple-find-my-airtag"),
                ble("apple_find_my", "apple-find-my-accessory"),
                ble("apple_find_my", "apple-find-my-airpods"),
                ble("apple_find_my", "apple-find-my-apple-device"),
                ble("apple_find_my", "apple-find-my"),
                ble("tile", "tile"),
                ble("smartthings_find", "samsung-smarttag"),
                ble(null, null),
            ).map { Labels.tracker(it) },
        )
    }
}

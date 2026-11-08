package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SelectionTest {
    private fun emitter(device: String, dbm: Double?, firstSeenNs: Long = 0) = Emitter(
        device = device, kind = "ble", band = "medium", latestDbm = dbm, strongestDbm = dbm,
        sightings = 3, address = "public", ssid = null, vendor = null, tracker = null,
        security = null, category = null, firstSeenNs = firstSeenNs,
    )

    @Test
    fun aTapTakesTheNearestDotWithinReach() {
        val spots = listOf(Spot("a", 0f, 0f), Spot("b", 10f, 0f))
        assertEquals("b", Selection.nearest(spots, 7f, 0f, 5f))
        assertNull(Selection.nearest(spots, 50f, 50f, 5f))
    }

    @Test
    fun theClosestStrengthComesFirstAndNoneComesLast() {
        val picked = emitter("ble:aa:aa:aa:00:00:01", -70.0)
        val all = listOf(
            emitter("ble:bb:bb:bb:00:00:01", -40.0),
            picked,
            emitter("ble:cc:cc:cc:00:00:01", null),
            emitter("ble:dd:dd:dd:00:00:01", -68.0),
            emitter("ble:ee:ee:ee:00:00:01", -75.0),
            emitter("ble:ff:ff:ff:00:00:01", -72.0),
        )
        assertEquals(
            listOf("dd", "ff", "ee", "bb", "cc"),
            Selection.similar(all, picked).map { it.device.substring(4, 6) },
        )
    }

    @Test
    fun withoutAStrengthTheLoudOrderStays() {
        val picked = emitter("ble:aa:aa:aa:00:00:01", null)
        val all = listOf(emitter("ble:bb:bb:bb:00:00:01", -40.0), picked, emitter("ble:cc:cc:cc:00:00:01", -90.0))
        assertEquals(listOf("bb", "cc"), Selection.similar(all, picked).map { it.device.substring(4, 6) })
    }

    @Test
    fun theCardSaysHowLongAgoItWasFirstHeard() {
        val card = Selection.card(emitter("ble:aa:aa:aa:00:00:01", -70.0, firstSeenNs = 1_000_000_000L), 181_000_000_000L)
        assertEquals("BLE  aa:aa:aa:00:00:01", card.first())
        assertEquals("Signal: latest -70 dBm, strongest -70 dBm (medium)", card[1])
        assertEquals("Heard 3 times, first 3 min ago", card.last())
    }
}

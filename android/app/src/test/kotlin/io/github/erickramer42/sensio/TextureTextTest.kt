package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Test

/** The texture rows, from what the core's `emitters` line gives. */
class TextureTextTest {
    private val s = 1_000_000_000L
    private val at = 1_000 * s

    private fun emitter(device: String, kind: String, firstSeenS: Long, address: String = "ble", tracker: String? = null) =
        Emitter(device, kind, "strong", -50.0, -50.0, 1, address, null, null, tracker, null, null, firstSeenNs = firstSeenS * s)

    private fun picture(emitters: List<Emitter>, texture: Texture) =
        Picture("parked", emitters, 0, 60, atNs = at, recentNs = 60 * s, texture = texture)

    @Test
    fun countsShowByKindWithTheNewOnesAndTheNewestNamed() {
        val p = picture(
            listOf(
                emitter("ble:aa:aa:aa:00:00:01", "ble", 990, tracker = "apple_find_my"),
                emitter("bt:dd:dd:dd:00:00:06", "bt_classic", 995, address = "global"),
            ),
            Texture(
                counts = mapOf("ble" to 3, "bt_classic" to 1, "access_point" to 0),
                new = mapOf("ble" to 1, "bt_classic" to 1),
                randomizedWifi = 0,
                newest = listOf("bt:dd:dd:dd:00:00:06", "ble:aa:aa:aa:00:00:01"),
            ),
        )
        assertEquals(
            listOf("Around, last 60 s: 3 BLE (1 new), 1 BT (1 new)", "Newest: BT dd:dd:dd:00:00:06; BLE Find My"),
            TextureText.lines(p, emptySet()),
        )
    }

    @Test
    fun enrolledDevicesAreTakenOutOfTheCountsAndTheNewest() {
        val p = picture(
            listOf(
                emitter("wifi:ee:ee:ee:00:00:01", "access_point", 990, address = "randomized"),
                emitter("wifi:cc:cc:cc:00:00:01", "access_point", 100, address = "global"),
            ),
            Texture(
                counts = mapOf("access_point" to 2),
                new = mapOf("access_point" to 1),
                randomizedWifi = 1,
                newest = listOf("wifi:ee:ee:ee:00:00:01", "wifi:cc:cc:cc:00:00:01"),
            ),
        )
        assertEquals(
            listOf("Around, last 60 s: 1 AP", "Newest: AP cc:cc:cc:00:00:01"),
            TextureText.lines(p, setOf("wifi:ee:ee:ee:00:00:01")),
        )
    }
}

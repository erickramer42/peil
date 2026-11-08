package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Test

/** The coverage rows, from what the core's `coverage` line gives. */
class CoverageTextTest {
    private val s = 1_000_000_000L

    private fun coverage(
        lastArrival: Map<String, Map<String, Long>>,
        radios: List<RadioCoverage>,
        bootNs: Long? = 100 * s,
    ) = Coverage(0, 0, 0, 0, 0, 0, bootNs, lastArrival, radios)

    @Test
    fun thePhonesRadiosShowTheirAgeStateAndLostTime() {
        val c = coverage(
            mapOf("phone" to mapOf("wifi_ap" to 96 * s, "ble_advert" to 100 * s, "gnss_fix" to 40 * s)),
            listOf(
                RadioCoverage("ble", "phone", "filtered", filteredS = 180, lostS = 0),
                RadioCoverage("bt_classic", "phone", "off", filteredS = 0, lostS = 100),
            ),
        )
        assertEquals(
            listOf(
                "Phone: Wi-Fi 4 s, BLE filtered 0 s (filtered 3 min), GNSS nothing for 60 s, Classic off (lost 100 s)",
                "Not watched: Wi-Fi stations (need the laptop), sub-GHz, cellular",
            ),
            CoverageText.lines(c),
        )
    }

    @Test
    fun aLinkedLaptopShowsWhatItGivesAndADownOneSaysSo() {
        val laptop = mapOf("wifi_client" to 99 * s, "ble_advert" to 50 * s)
        val linked = coverage(mapOf("laptop" to laptop), listOf(RadioCoverage("link", "laptop", "on", 0, 0)))
        assertEquals(
            listOf(
                "Phone: Wi-Fi nothing yet, BLE nothing yet, GNSS nothing yet, Classic nothing yet",
                "laptop: Wi-Fi 1 s, BLE nothing for 50 s",
                "Not watched: sub-GHz, cellular",
            ),
            CoverageText.lines(linked),
        )
        val down = coverage(mapOf("laptop" to laptop), listOf(RadioCoverage("link", "laptop", "off", 0, lostS = 30)))
        assertEquals("laptop: link down: Wi-Fi 1 s, BLE nothing for 50 s, lost 30 s", CoverageText.lines(down)[1])
    }
}

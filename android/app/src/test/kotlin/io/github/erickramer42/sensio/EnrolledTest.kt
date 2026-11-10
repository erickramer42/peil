package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** What the Mine list offers and which suggestions start checked. */
class EnrolledTest {
    private fun emitter(device: String, band: String, dbm: Double, tracker: String? = null) =
        Emitter(device, "ble", band, dbm, dbm, 1, "ble", null, null, tracker, null, null)

    private fun picture(emitters: List<Emitter>) = Picture("parked", emitters, 0, 60)

    private fun loud(n: Int) = (1..n).map { emitter("ble:aa:aa:aa:00:00:%02x".format(it), "strong", -40.0 - it) }

    @Test
    fun aWeakTrackerIsOfferedPastTheLimitOfTheLoudOnes() {
        val tag = emitter("ble:bb:bb:bb:00:00:01", "weak", -95.0, tracker = "tile")
        val offer = Enrolled.offer(picture(loud(5) + tag), emptySet(), max = 3)
        assertEquals(
            listOf("ble:bb:bb:bb:00:00:01", "ble:aa:aa:aa:00:00:01", "ble:aa:aa:aa:00:00:02", "ble:aa:aa:aa:00:00:03"),
            offer.map { it.second },
        )
    }

    @Test
    fun theEnrolledComeFirstEvenWhenNotHeard() {
        val offer = Enrolled.offer(picture(loud(2)), setOf("ble:aa:aa:aa:00:00:02", "ble:cc:cc:cc:00:00:01"))
        assertEquals(
            listOf("ble:aa:aa:aa:00:00:02", "ble:cc:cc:cc:00:00:01", "ble:aa:aa:aa:00:00:01"),
            offer.map { it.second },
        )
        assertEquals(null, offer[1].first)
    }

    @Test
    fun theDefaultLimitIsFortyLoudOnes() {
        assertEquals(40, Enrolled.offer(picture(loud(50)), emptySet()).size)
    }

    @Test
    fun noPictureOffersOnlyTheEnrolled() {
        assertEquals(listOf("ble:cc:cc:cc:00:00:01"), Enrolled.offer(null, setOf("ble:cc:cc:cc:00:00:01")).map { it.second })
    }

    @Test
    fun aReceiversOwnAddressesComeAfterTheEnrolledHeardOrNot() {
        val laptop = emitter("ble:aa:aa:aa:00:00:02", "strong", -42.0)
        val labeled = Emitter("wifi:bb:bb:bb:00:00:01", "station", "unknown", null, null, 1, "wifi", null, null, null, null, null, own = "laptop")
        val offer = Enrolled.offer(
            picture(loud(1) + laptop + labeled),
            setOf("ble:cc:cc:cc:00:00:01"),
            own = mapOf("ble:aa:aa:aa:00:00:02" to "laptop", "bt:aa:aa:aa:00:00:02" to "laptop"),
        )
        assertEquals(
            "each once, the unheard one too",
            listOf(
                "ble:cc:cc:cc:00:00:01",
                "ble:aa:aa:aa:00:00:02",
                "bt:aa:aa:aa:00:00:02",
                "wifi:bb:bb:bb:00:00:01",
                "ble:aa:aa:aa:00:00:01",
            ),
            offer.map { it.second },
        )
        assertEquals("laptop", Enrolled.ownOf("wifi:bb:bb:bb:00:00:01", labeled, emptyMap()))
        assertEquals(null, Enrolled.ownOf("ble:aa:aa:aa:00:00:01", loud(1)[0], emptyMap()))
    }

    @Test
    fun aReceiversOwnAddressNeverStartsChecked() {
        val strong = emitter("ble:aa:aa:aa:00:00:01", "strong", -40.0)
        assertFalse(Enrolled.suggested(strong, emptySet(), own = mapOf("ble:aa:aa:aa:00:00:01" to "laptop")))
        val labeled = Emitter("ble:aa:aa:aa:00:00:01", "ble", "strong", -40.0, -40.0, 1, "ble", null, null, null, null, null, own = "laptop")
        assertFalse(Enrolled.suggested(labeled, emptySet()))
    }

    @Test
    fun onlyStrongNonTrackersStartCheckedAndOnlyBeforeAnythingIsEnrolled() {
        val strong = emitter("ble:aa:aa:aa:00:00:01", "strong", -40.0)
        val strongTag = emitter("ble:bb:bb:bb:00:00:01", "strong", -40.0, tracker = "tile")
        val medium = emitter("ble:aa:aa:aa:00:00:02", "medium", -70.0)
        assertTrue(Enrolled.suggested(strong, emptySet()))
        assertFalse(Enrolled.suggested(strongTag, emptySet()))
        assertFalse(Enrolled.suggested(medium, emptySet()))
        assertFalse(Enrolled.suggested(strong, setOf("ble:cc:cc:cc:00:00:01")))
        assertFalse(Enrolled.suggested(null, emptySet()))
    }
}

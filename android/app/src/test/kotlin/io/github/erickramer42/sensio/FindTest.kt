package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Test

/** Find mode on the phone: the choices, the bearing turned to the phone, and the status row. */
class FindTest {
    private fun emitter(device: String, kind: String, dbm: Double?, tracker: String? = null) =
        Emitter(device, kind, "strong", dbm, dbm, 1, "ble", null, null, tracker, null, null)

    private fun answer(status: String, covered: Int = 0, bearing: Double? = null, contrast: Double? = null, unpaired: Long = 0) =
        FindAnswer("ble:aa:aa:aa:00:00:01", status, bearing, contrast, covered.toLong(), unpaired, -62.0, List(12) { it < covered })

    @Test
    fun choicesAreHeardBleEmittersWithTheSelectedThenTrackersThenTheLoudestFirst() {
        val emitters = listOf(
            emitter("ble:aa:aa:aa:00:00:01", "ble", -50.0),
            emitter("wifi:cc:cc:cc:00:00:02", "access_point", -40.0),
            emitter("ble:bb:bb:bb:00:00:03", "ble", -80.0, tracker = "apple_find_my"),
            emitter("ble:dd:dd:dd:00:00:04", "ble", -70.0),
            emitter("ble:ee:ee:ee:00:00:05", "ble", null),
        )
        assertEquals(
            listOf("ble:bb:bb:bb:00:00:03", "ble:aa:aa:aa:00:00:01", "ble:dd:dd:dd:00:00:04", "ble:ee:ee:ee:00:00:05"),
            Find.candidates(emitters, null).map { it.device },
        )
        assertEquals("ble:dd:dd:dd:00:00:04", Find.candidates(emitters, "ble:dd:dd:dd:00:00:04").first().device)
    }

    @Test
    fun aBearingIsTurnedToThePhonesTop() {
        assertEquals(0.0, Find.relative(105.0, 105.0), 0.0)
        assertEquals(90.0, Find.relative(105.0, 15.0), 0.0)
        assertEquals(270.0, Find.relative(15.0, 105.0), 0.0)
        assertEquals(30.0, Find.relative(10.0, 340.0), 0.0)
    }

    @Test
    fun theStatusSaysWhatToDoNext() {
        val label = "aa:aa:aa:00:00:01"
        assertEquals("Finding $label: hold the phone in front of you and turn slowly in place.", Find.line(null, label))
        assertEquals(
            "Finding $label: heard, but not with a heading: hold the phone flatter and turn slowly, latest -62 dBm.",
            Find.line(answer("no_readings", unpaired = 3), label),
        )
        assertEquals(
            "Finding $label: keep turning: 4 of 12 directions heard, latest -62 dBm.",
            Find.line(answer("turn", covered = 4), label),
        )
        assertEquals(
            "Finding $label: no direction stands out: turn once more, slowly, or move a few meters, latest -62 dBm.",
            Find.line(answer("unclear", covered = 12), label),
        )
        assertEquals(
            "Finding $label: strongest toward the wedge, 8 dB above the rest, latest -62 dBm.",
            Find.line(answer("found", covered = 12, bearing = 105.0, contrast = 7.6), label),
        )
    }
}

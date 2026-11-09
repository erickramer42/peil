package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Where a dot sits on the ring: by strength on one scale, and the rim only for no strength. */
class RingScaleTest {
    private fun ble(band: String, dbm: Double?) =
        Emitter("ble:aa:aa:aa:00:00:01", "ble", band, dbm, dbm, 1, "ble", null, null, null, null, null)

    @Test
    fun strengthRunsFromTheCenterToTheEdgeAndIsHeldToItsEnds() {
        assertEquals(RingScale.INNER, RingScale.fraction(-40, rim = false), 1e-6f)
        assertEquals(RingScale.INNER, RingScale.fraction(-25, rim = false), 1e-6f)
        assertEquals(1f, RingScale.fraction(-100, rim = false), 1e-6f)
        assertEquals(1f, RingScale.fraction(-110, rim = false), 1e-6f)
        // -80 dBm sits two thirds of the way out, so the weakest band has the outer third.
        assertEquals(RingScale.INNER + (1f - RingScale.INNER) * 2f / 3f, RingScale.fraction(-80, rim = false), 1e-6f)
        assertTrue(RingScale.fraction(-82, rim = false) < RingScale.fraction(-98, rim = false))
    }

    @Test
    fun theRimTakesTheEdgeOnlyWhileSomethingHasNoStrength() {
        assertEquals(RingScale.RIM, RingScale.fraction(-100, rim = true), 1e-6f)
        assertTrue(RingScale.onRim(ble("unknown", null)))
        assertFalse(RingScale.onRim(ble("weak", -95.0)))
        val onRim = RingScale.place(ble("unknown", null), rim = true, spread = 0.5f)
        assertTrue(onRim > RingScale.RIM && onRim < 1f)
        assertEquals(RingScale.fraction(-95, rim = true), RingScale.place(ble("weak", -95.0), rim = true, spread = 0.5f), 1e-6f)
    }
}

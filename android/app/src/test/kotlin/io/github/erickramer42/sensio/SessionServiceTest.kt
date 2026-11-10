package io.github.erickramer42.sensio

import org.junit.Assert.assertTrue
import org.junit.Test

/** The screen-off BLE filters, against the formats the describe layer recognizes. */
class SessionServiceTest {
    @Test
    fun theScreenOffFilterHoldsEveryTileService() {
        // describe.rs's `tile` format: service data under 0xFEED, 0xFEEC or 0xFD84.
        for (uuid in listOf(0xFEED, 0xFEEC, 0xFD84)) {
            assertTrue("%04X".format(uuid), uuid in SessionService.SCREEN_OFF_SERVICES)
        }
    }

    @Test
    fun theScreenOffFilterHoldsRemoteId() {
        // remote_id.rs: Open Drone ID's service data under 0xFFFA.
        assertTrue(0xFFFA in SessionService.SCREEN_OFF_SERVICES)
    }
}

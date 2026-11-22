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

    @Test
    fun anAircraftsCardGivesItsDbfsAnnouncementsDistanceAndRegistration() {
        val air = Emitter(
            device = "adsb:bbbb01", kind = "aircraft", band = "unknown", latestDbm = null, strongestDbm = null,
            sightings = 2, address = "aircraft", ssid = null, vendor = null, tracker = null, security = null,
            category = "rotorcraft",
            aircraft = Aircraft(
                callsign = "TEST01", altBaroFt = 3500, groundSpeedKt = 120.4, trackDeg = 271.2,
                distanceM = 1111.95, bearingDeg = 0.0, squawk = "1200", signalDbfs = -21.4,
                registry = Registration(found = true, nNumber = "N1A", make = "MAKER", model = "MODEL", year = 2001, owner = "AN AGENCY"),
            ),
        )
        assertEquals(
            listOf(
                "AIR  bbbb01",
                "Signal: -21.4 dBFS, on no dBm scale",
                "Category: rotorcraft",
                "Callsign: TEST01",
                "Altitude: 3500 ft",
                "Speed: 120 kt, track 271°",
                "From here: 1.1 km, bearing 0°",
                "Squawk: 1200",
                "Registered: N1A, MAKER MODEL, 2001",
                "Registrant: AN AGENCY",
                "Heard 2 times",
            ),
            Selection.card(air, 0),
        )
        val unlisted = Aircraft(onGround = true, trackDeg = 90.0, registry = Registration(found = false))
        assertEquals(
            listOf("On the ground", "Track 90°", "Registry: not listed (withheld, a privacy address, or newer than the snapshot)"),
            Selection.aircraft(unlisted),
        )
    }

    @Test
    fun aDronesCardPlacesItAndItsOperatorFromTheVehicle() {
        val drone = Drone(
            uasId = "TEST000001", status = 2, heightM = 40.0, speedMps = 5.2,
            distanceM = 300.0, bearingDeg = 45.0, operatorDistanceM = 120.0, operatorBearingDeg = null, operatorId = "OPTEST1",
        )
        assertEquals(
            listOf(
                "Drone ID: TEST000001",
                "Status: airborne",
                "Height 40 m, speed 5 m/s",
                "Drone from here: 300 m, bearing 45°",
                "Operator from here: 120 m",
                "Operator ID: OPTEST1",
            ),
            Selection.drone(drone),
        )
        assertNull(Selection.away(null, 10.0))
    }
}

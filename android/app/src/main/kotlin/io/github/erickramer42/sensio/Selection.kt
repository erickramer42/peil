package io.github.erickramer42.sensio

import kotlin.math.abs
import kotlin.math.hypot

/** A dot on the ring: the emitter it stands for and where it was drawn. */
class Spot(val device: String, val x: Float, val y: Float)

/**
 * An emitter tapped on the ring: its card, and the others ordered by how close
 * their signal strength is to its own. Equal strength isn't equal distance:
 * transmit power, antennas and what's in the way differ by tens of dB between
 * emitters, so the order says nothing about where an emitter is.
 */
object Selection {
    /** The dot nearest (x, y) within [reach], or null when none is that close. */
    fun nearest(spots: List<Spot>, x: Float, y: Float, reach: Float): String? =
        spots.map { it to hypot(it.x - x, it.y - y) }
            .filter { it.second <= reach }
            .minByOrNull { it.second }
            ?.first?.device

    /**
     * The other emitters, the closest in latest signal strength first; those
     * without one last. Ties keep the core's order, loud first, and so does
     * everything when [selected] has no strength to compare.
     */
    fun similar(emitters: List<Emitter>, selected: Emitter): List<Emitter> {
        val others = emitters.filter { it.device != selected.device }
        val dbm = selected.latestDbm ?: return others
        return others.sortedWith(compareBy({ it.latestDbm == null }, { it.latestDbm?.let { d -> abs(d - dbm) } ?: 0.0 }))
    }

    /** What the describe layer says about [e], a line per fact, for the card. */
    fun card(e: Emitter, atNs: Long): List<String> = buildList {
        val kind = Labels.KINDS[e.kind] ?: e.kind
        add(listOfNotNull(kind, e.device.substringAfter(':'), Labels.tracker(e)).joinToString("  "))
        val strength = listOfNotNull(
            e.latestDbm?.let { "latest %.0f dBm".format(it) },
            e.strongestDbm?.let { "strongest %.0f dBm".format(it) },
        ).ifEmpty { listOf("no dBm") }
        val dbfs = e.aircraft?.signalDbfs
        add(if (dbfs != null) "Signal: %.1f dBFS, on no dBm scale".format(dbfs) else "Signal: ${strength.joinToString(", ")} (${e.band})")
        e.ssid?.let { add("Network: \"$it\"") }
        e.vendor?.let { add("Vendor: $it") }
        e.category?.let { add("Category: $it") }
        e.aircraft?.let { addAll(aircraft(it)) }
        e.drone?.let { addAll(drone(it)) }
        e.security?.let { add("Security: $it") }
        if (e.address == "randomized") add("Address: randomized")
        e.own?.let { add("Own address of: $it (as it reported over the link)") }
        val heard = "Heard ${e.sightings} time${if (e.sightings == 1L) "" else "s"}"
        add(if (e.firstSeenNs in 1..atNs) "$heard, first ${ago(atNs - e.firstSeenNs)} ago" else heard)
    }

    /** An aircraft's card lines, in the units it announces (feet, knots); bearings from true north. */
    fun aircraft(a: Aircraft): List<String> = buildList {
        a.callsign?.let { add("Callsign: $it") }
        when {
            a.onGround -> add("On the ground")
            a.altBaroFt != null -> add("Altitude: ${a.altBaroFt} ft")
        }
        val track = a.trackDeg?.let { "track %.0f°".format(it) }
        a.groundSpeedKt?.let { add(listOfNotNull("Speed: %.0f kt".format(it), track).joinToString(", ")) }
            ?: track?.let { add(it.replaceFirstChar { c -> c.uppercase() }) }
        away(a.distanceM, a.bearingDeg)?.let { add("From here: $it") }
        a.squawk?.let { add("Squawk: $it") }
        a.emergency?.let { add("Emergency: $it") }
        a.registry?.let { r ->
            if (!r.found) {
                add("Registry: not listed (withheld, a privacy address, or newer than the snapshot)")
            } else {
                add("Registered: " + listOfNotNull(r.nNumber, listOfNotNull(r.make, r.model).joinToString(" ").ifEmpty { null }, r.year?.toString()).joinToString(", "))
                r.owner?.let { add("Registrant: $it") }
            }
        }
    }

    /** A drone's card lines from its Remote ID messages; bearings from true north. */
    fun drone(d: Drone): List<String> = buildList {
        d.uasId?.let { add("Drone ID: $it") }
        DRONE_STATUS[d.status]?.let { add("Status: $it") }
        val motion = listOfNotNull(d.heightM?.let { "height %.0f m".format(it) }, d.speedMps?.let { "speed %.0f m/s".format(it) })
        if (motion.isNotEmpty()) add(motion.joinToString(", ").replaceFirstChar { it.uppercase() })
        away(d.distanceM, d.bearingDeg)?.let { add("Drone from here: $it") }
        away(d.operatorDistanceM, d.operatorBearingDeg)?.let { add("Operator from here: $it") }
        d.operatorId?.let { add("Operator ID: $it") }
    }

    private val DRONE_STATUS = mapOf(1L to "on the ground", 2L to "airborne", 3L to "emergency", 4L to "Remote ID failure")

    /** Distance and bearing from the vehicle's latest fix, or null without a distance. */
    fun away(distanceM: Double?, bearingDeg: Double?): String? {
        val m = distanceM ?: return null
        val far = if (m < 1000) "%.0f m".format(m) else "%.1f km".format(m / 1000)
        return bearingDeg?.let { "$far, bearing %.0f°".format(it) } ?: far
    }

    fun ago(ns: Long): String {
        val s = ns / 1_000_000_000L
        return when {
            s < 120 -> "$s s"
            s < 2 * 3600 -> "${s / 60} min"
            else -> "${s / 3600} h"
        }
    }
}

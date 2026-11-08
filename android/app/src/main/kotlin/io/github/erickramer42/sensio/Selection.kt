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
        add(listOfNotNull(kind, e.device.substringAfter(':'), e.tracker?.let { Labels.TRACKERS[it] ?: it }).joinToString("  "))
        val strength = listOfNotNull(
            e.latestDbm?.let { "latest %.0f dBm".format(it) },
            e.strongestDbm?.let { "strongest %.0f dBm".format(it) },
        ).ifEmpty { listOf("no dBm") }
        add("Signal: ${strength.joinToString(", ")} (${e.band})")
        e.ssid?.let { add("Network: \"$it\"") }
        e.vendor?.let { add("Vendor: $it") }
        e.category?.let { add("Category: $it") }
        e.security?.let { add("Security: $it") }
        if (e.address == "randomized") add("Address: randomized")
        val heard = "Heard ${e.sightings} time${if (e.sightings == 1L) "" else "s"}"
        add(if (e.firstSeenNs in 1..atNs) "$heard, first ${ago(atNs - e.firstSeenNs)} ago" else heard)
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

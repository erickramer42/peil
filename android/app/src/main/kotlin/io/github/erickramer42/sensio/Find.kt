package io.github.erickramer42.sensio

/**
 * Find mode's answer, as the core gives it in a `find` line (ADR-0005 as
 * amended 2026-11-09): how far the sweep has got, the bearing once one sector
 * stands out, and which of the sweep's directions have readings, from north
 * clockwise.
 */
class FindAnswer(
    val device: String,
    val status: String,
    val bearingDeg: Double?,
    val contrastDb: Double?,
    val paired: Long,
    val unpaired: Long,
    val latestDbm: Double?,
    val covered: List<Boolean>,
)

/**
 * Find mode on the phone (ADR-0004 section 3): the user picks a BLE emitter,
 * turns slowly in place with the phone held in front, and the ring shows the
 * direction the core heard it strongest. The core estimates; this only orders
 * the choices, turns the bearing to the phone's own heading, and words the
 * status.
 */
object Find {
    /** The most emitters offered to find. */
    const val LISTED = 40

    /** Heard BLE emitters to find: the selected one first, then trackers, then the loudest. */
    fun candidates(emitters: List<Emitter>, selected: String?): List<Emitter> =
        emitters.filter { it.kind == "ble" }
            .sortedWith(compareBy({ it.device != selected }, { it.tracker == null }, { -(it.latestDbm ?: -200.0) }))
            .take(LISTED)

    /** A bearing as the phone sees it: degrees clockwise from the phone's top, 0 to under 360. */
    fun relative(bearingDeg: Double, headingDeg: Double): Double {
        val d = (bearingDeg - headingDeg) % 360.0
        return if (d < 0) d + 360.0 else d
    }

    /** The status row while finding [label]. */
    fun line(answer: FindAnswer?, label: String): String {
        val heard = answer?.latestDbm?.let { ", latest %.0f dBm".format(it) } ?: ""
        val what = when (answer?.status) {
            null -> "hold the phone in front of you and turn slowly in place"
            "no_readings" ->
                if ((answer.unpaired) > 0) "heard, but not with a heading: hold the phone flatter and turn slowly"
                else "not heard yet: turn slowly in place"
            "turn" -> "keep turning: ${answer.covered.count { it }} of ${answer.covered.size} directions heard"
            "unclear" -> "no direction stands out: turn once more, slowly, or move a few meters"
            "found" -> "strongest toward the wedge" +
                (answer.contrastDb?.let { ", %.0f dB above the rest".format(it) } ?: "")
            else -> answer.status
        }
        return "Finding $label: $what$heard."
    }
}

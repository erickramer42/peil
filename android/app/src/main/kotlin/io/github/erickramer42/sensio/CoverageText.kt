package io.github.erickramer42.sensio

/** One radio's state from the core's `coverage` line (ADR-0005). */
class RadioCoverage(val radio: String, val receiver: String, val state: String, val filteredS: Long, val lostS: Long)

/**
 * The core's `coverage` line: totals over every source, the time it covers
 * up to ([bootNs]), each receiver's latest arrival by event kind, and the
 * radios' states.
 */
class Coverage(
    val lines: Long,
    val events: Long,
    val rejected: Long,
    val withheld: Long,
    val dropped: Long,
    val sleptS: Long,
    val bootNs: Long? = null,
    /** Receiver to event kind to its latest arrival, on the boot clock. */
    val lastArrival: Map<String, Map<String, Long>> = emptyMap(),
    val radios: List<RadioCoverage> = emptyList(),
)

/**
 * What the coverage line says, one line per receiver: which radios are up,
 * how long since each last gave anything, and what isn't watched. Ages are
 * as of the core's last answer, so a replay shows the same.
 *
 * Plain Kotlin with no Android types, so the JVM unit tests cover it.
 */
object CoverageText {
    /** A radio the screen names, the event kinds it gives, and how long without one is worth saying. */
    private class Watch(val radio: String, val label: String, val kinds: List<String>, val quietS: Long)

    /** The phone's radios, from SessionService: Wi-Fi scans every 10 s, Classic rounds every 30 s, GNSS at 1 Hz. */
    private val PHONE = listOf(
        Watch("wifi", "Wi-Fi", listOf("wifi_ap"), 30),
        Watch("ble", "BLE", listOf("ble_advert"), 15),
        Watch("gnss", "GNSS", listOf("gnss_fix"), 10),
        Watch("bt_classic", "Classic", listOf("bt_classic"), 90),
    )

    /** What a laptop's Kismet gives; shown once it has given any. */
    private val LAPTOP = listOf(
        Watch("wifi", "Wi-Fi", listOf("wifi_ap", "wifi_client"), 30),
        Watch("ble", "BLE", listOf("ble_advert"), 30),
        Watch("bt_classic", "Classic", listOf("bt_classic"), 90),
    )

    fun lines(c: Coverage): List<String> {
        val out = mutableListOf<String>()
        out += "Phone: " + PHONE.joinToString(", ") { watch(c, Feed.RECEIVER, it) }
        val laptops = (c.radios.filter { it.radio == "link" }.map { it.receiver } + c.lastArrival.keys)
            .filter { it != Feed.RECEIVER }
            .distinct()
            .sorted()
        for (laptop in laptops) {
            val link = c.radio("link", laptop)
            val head = when (link?.state) {
                null, "on" -> laptop
                else -> "$laptop: link down"
            }
            val seen = LAPTOP.filter { w -> w.kinds.any { c.lastArrival[laptop]?.containsKey(it) == true } }
            val parts = seen.map { watch(c, laptop, it) } + listOfNotNull(link?.let { totals(it) })
            out += if (parts.isEmpty()) head else "$head: " + parts.joinToString(", ")
        }
        val stations = laptops.none { c.radio("link", it)?.state == "on" }
        out += "Not watched: " + listOfNotNull(if (stations) "Wi-Fi stations (need the laptop)" else null, "sub-GHz", "cellular").joinToString(", ")
        return out
    }

    private fun Coverage.radio(radio: String, receiver: String) = radios.firstOrNull { it.radio == radio && it.receiver == receiver }

    /** One radio: its state when it isn't plainly on, its age, and its time short of on. */
    private fun watch(c: Coverage, receiver: String, w: Watch): String {
        val radio = c.radio(w.radio, receiver)
        val state = when (radio?.state) {
            "off" -> " off"
            "failed" -> " failing"
            "filtered" -> " filtered"
            else -> ""
        }
        val last = w.kinds.mapNotNull { c.lastArrival[receiver]?.get(it) }.maxOrNull()
        val age = when {
            radio?.state == "off" || radio?.state == "failed" -> ""
            last == null || c.bootNs == null -> " nothing yet"
            else -> {
                val s = maxOf(0L, (c.bootNs - last) / 1_000_000_000L)
                if (s >= w.quietS) " nothing for ${duration(s)}" else " $s s"
            }
        }
        val totals = radio?.let { totals(it) }?.let { " ($it)" } ?: ""
        return "${w.label}$state$age$totals"
    }

    /** Time a radio spent filtered or lost, when there was any. */
    private fun totals(r: RadioCoverage): String? = listOfNotNull(
        if (r.filteredS > 0) "filtered ${duration(r.filteredS)}" else null,
        if (r.lostS > 0) "lost ${duration(r.lostS)}" else null,
    ).joinToString(", ").ifEmpty { null }

    fun duration(s: Long): String = if (s < 120) "$s s" else "${s / 60} min"
}

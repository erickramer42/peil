package io.github.erickramer42.sensio

/**
 * The ambient texture's rows under the ring: how many emitters of each kind
 * the core saw in its recent window, how many of them are new in it, and the
 * newest few. The user's enrolled devices are taken out, as the list does.
 */
object TextureText {
    private val ORDER = listOf("access_point", "station", "ble", "bt_classic", "tpms")

    fun lines(picture: Picture, enrolled: Set<String>): List<String> {
        val texture = picture.texture ?: return emptyList()
        val hidden = picture.emitters.filter { it.device in enrolled }
        val recentFrom = picture.atNs - picture.recentNs
        val parts = ORDER.mapNotNull { kind ->
            val mine = hidden.filter { it.kind == kind }
            val count = ((texture.counts[kind] ?: 0) - mine.size).coerceAtLeast(0)
            val new = ((texture.new[kind] ?: 0) - mine.count { it.firstSeenNs >= recentFrom }).coerceAtLeast(0)
            when {
                count == 0L -> null
                new > 0 -> "$count ${label(kind)} ($new new)"
                else -> "$count ${label(kind)}"
            }
        }
        val randomized = (texture.randomizedWifi - hidden.count { it.address == "randomized" }).coerceAtLeast(0)
        val around = buildString {
            append("Around, last ${picture.recentS} s: ")
            append(if (parts.isEmpty()) "nothing" else parts.joinToString(", "))
            if (randomized > 0) append("; $randomized randomized Wi-Fi")
        }
        val byDevice = picture.emitters.associateBy { it.device }
        val newest = texture.newest.filter { it !in enrolled }.map { device ->
            val e = byDevice[device]
            val what = e?.let {
                it.tracker?.let { t -> Labels.TRACKERS[t] ?: t }
                    ?: it.category
                    ?: it.ssid?.let { s -> "\"$s\"" }
                    ?: it.vendor
            } ?: device.substringAfter(':')
            e?.let { "${label(it.kind)} $what" } ?: what
        }
        return if (newest.isEmpty()) listOf(around) else listOf(around, "Newest: " + newest.joinToString("; "))
    }

    private fun label(kind: String) = Labels.KINDS[kind] ?: kind
}

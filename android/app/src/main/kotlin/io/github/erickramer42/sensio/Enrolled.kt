package io.github.erickramer42.sensio

import android.content.Context

/**
 * The user's own emitters (roadmap item 48): their phone, the car's radios,
 * their OBD adapter. The user picks them; the app only suggests, loudest
 * first, since the strongest emitters are most often the car's own. An
 * enrolled emitter is left off the ring and the list, and counted. The set
 * stays on the phone, in the app's private preferences.
 */
object Enrolled {
    private const val PREFS = "enrolled"
    private const val DEVICES = "devices"

    fun devices(context: Context): Set<String> =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getStringSet(DEVICES, emptySet()).orEmpty().toSet()

    fun set(context: Context, devices: Set<String>) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putStringSet(DEVICES, devices).apply()

    /**
     * What to offer: the enrolled ones first, then those a linked receiver
     * gave as its own ([own], device to receiver, and any emitter the core
     * labeled so), heard or not, then every tracker the core recognized,
     * loudest first, then the loudest others, at most [max] of them. Trackers
     * come whatever their strength: a tag in a wallet or a bag is often weak,
     * and the user can only mark what's offered.
     */
    fun offer(
        picture: Picture?,
        enrolled: Set<String>,
        own: Map<String, String> = emptyMap(),
        max: Int = 40,
    ): List<Pair<Emitter?, String>> {
        val emitters = picture?.emitters.orEmpty()
        val byDevice = emitters.associateBy { it.device }
        val mine = enrolled.sorted().map { byDevice[it] to it }
        val owned = (own.keys + emitters.filter { it.own != null }.map { it.device })
            .filter { it !in enrolled }.distinct().sorted()
        val (trackers, rest) = emitters.filter { it.device !in enrolled && it.device !in owned }.partition { it.tracker != null }
        return mine + owned.map { byDevice[it] to it } + (trackers + rest.take(max)).map { it to it.device }
    }

    /** The receiver [device] is the own address of, from the link or the core's label, or null. */
    fun ownOf(device: String, emitter: Emitter?, own: Map<String, String>): String? = own[device] ?: emitter?.own

    /**
     * Whether an offer starts checked: only when nothing is enrolled yet, and
     * then only strong ones that aren't trackers and that no receiver gave as
     * its own. A tracker hidden in the car is loud too, and enrolling it
     * would hide what Peil looks for; a receiver's own address is left for
     * the user to check, since another device can copy it.
     */
    fun suggested(emitter: Emitter?, enrolled: Set<String>, own: Map<String, String> = emptyMap()): Boolean =
        enrolled.isEmpty() && emitter != null && emitter.band == "strong" && emitter.tracker == null &&
            ownOf(emitter.device, emitter, own) == null
}

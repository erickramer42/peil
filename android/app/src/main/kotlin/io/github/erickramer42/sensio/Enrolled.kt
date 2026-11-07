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
     * What to offer: the enrolled ones first, then the loudest others. A
     * suggestion is checked only when nothing is enrolled yet, and then only
     * strong ones that aren't trackers: a tracker hidden in the car is loud
     * too, and enrolling it would hide what Peil looks for.
     */
    fun offer(picture: Picture?, enrolled: Set<String>, max: Int = 20): List<Pair<Emitter?, String>> {
        val emitters = picture?.emitters.orEmpty()
        val byDevice = emitters.associateBy { it.device }
        val mine = enrolled.sorted().map { byDevice[it] to it }
        val others = emitters.filter { it.device !in enrolled }.take(max).map { it to it.device }
        return mine + others
    }

    fun suggested(emitter: Emitter?, enrolled: Set<String>): Boolean =
        enrolled.isEmpty() && emitter != null && emitter.band == "strong" && emitter.tracker == null
}

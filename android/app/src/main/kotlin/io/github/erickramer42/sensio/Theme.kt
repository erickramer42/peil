package io.github.erickramer42.sensio

import kotlin.math.pow

/**
 * A theme's colors by role, as ARGB. Views read roles and never hold a color
 * of their own, so a theme is one more palette. All three themes are dark;
 * Low glare and Night keep the light down for a car at night: near-black
 * backgrounds, amber or red text, no white, and no dot brighter than the text.
 */
class Palette(
    /** The window. */
    val bg: Int,
    /** Cards and dialogs. */
    val surface: Int,
    /** Buttons. */
    val elevated: Int,
    /** A pressed button. */
    val elevatedLight: Int,
    /** Control borders and the ring's circles. */
    val outline: Int,
    /** The ring's sector lines, quieter than its circles. */
    val faint: Int,
    val text: Int,
    val textDim: Int,
    /** A tracker; on the ring it always has a halo too, since in Night nearly everything is red. */
    val danger: Int,
    /** Each emitter kind's dot. */
    val kinds: Map<String, Int>,
)

private fun argb(hex: Long): Int = (0xFF000000L or hex).toInt()

/** The themes, with the keys they're stored under; an unknown key reads as Dark. */
enum class Theme(val key: String, val label: String, val palette: Palette) {
    DARK(
        "dark", "Dark",
        Palette(
            bg = argb(0x141413), surface = argb(0x1d1d1b), elevated = argb(0x292926), elevatedLight = argb(0x33332f),
            outline = argb(0x6b6b63), faint = argb(0x3a3a35), text = argb(0xecebe5), textDim = argb(0x9c9b92),
            danger = argb(0xf87171),
            kinds = mapOf(
                "ble" to argb(0x50c878), "bt_classic" to argb(0xf0b43c), "access_point" to argb(0x5aa0ff),
                "station" to argb(0x50dce6),
            ),
        ),
    ),
    LOW_GLARE(
        "lowGlare", "Low glare",
        Palette(
            bg = argb(0x0d0a07), surface = argb(0x18120c), elevated = argb(0x261c12), elevatedLight = argb(0x31241a),
            outline = argb(0x85694a), faint = argb(0x4a3a28), text = argb(0xf3c895), textDim = argb(0xb58f64),
            danger = argb(0xff4a38),
            kinds = mapOf(
                "ble" to argb(0xd17d00), "bt_classic" to argb(0x9e9147), "access_point" to argb(0xffb299),
                "station" to argb(0xb89c00),
            ),
        ),
    ),
    NIGHT(
        "night", "Night",
        Palette(
            bg = argb(0x080404), surface = argb(0x130909), elevated = argb(0x201010), elevatedLight = argb(0x2b1616),
            outline = argb(0x934a44), faint = argb(0x4a2422), text = argb(0xff9484), textDim = argb(0xc46a5e),
            danger = argb(0xff3030),
            kinds = mapOf(
                "ble" to argb(0xff8000), "bt_classic" to argb(0xff7373), "access_point" to argb(0x9e7777),
                "station" to argb(0xb86253),
            ),
        ),
    ),
    ;

    fun next(): Theme = entries[(ordinal + 1) % entries.size]

    companion object {
        fun of(key: String?): Theme = entries.firstOrNull { it.key == key } ?: DARK
    }
}

/** WCAG 2's contrast ratio between two opaque colors, from 1 to 21. */
object Contrast {
    fun ratio(a: Int, b: Int): Double {
        val (hi, lo) = listOf(luminance(a), luminance(b)).sortedDescending()
        return (hi + 0.05) / (lo + 0.05)
    }

    private fun luminance(argb: Int): Double {
        fun channel(shift: Int): Double {
            val c = ((argb shr shift) and 0xff) / 255.0
            return if (c <= 0.03928) c / 12.92 else ((c + 0.055) / 1.055).pow(2.4)
        }
        return 0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }
}

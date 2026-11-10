package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Every theme keeps its text readable and its marks visible (WCAG 2.1, 1.4.3 and 1.4.11). */
class ThemeTest {
    private fun check(what: String, fg: Int, bg: Int, floor: Double, theme: Theme) {
        val ratio = Contrast.ratio(fg, bg)
        assertTrue("${theme.label}: $what is %.2f:1, under %.1f:1".format(ratio, floor), ratio >= floor)
    }

    @Test
    fun textHoldsFourAndAHalfToOneWhereverItSits() {
        for (theme in Theme.entries) {
            val p = theme.palette
            for ((name, under) in listOf("bg" to p.bg, "surface" to p.surface, "elevated" to p.elevated, "pressed" to p.elevatedLight)) {
                check("text on $name", p.text, under, 4.5, theme)
            }
            for ((name, under) in listOf("bg" to p.bg, "surface" to p.surface)) {
                check("dim text on $name", p.textDim, under, 4.5, theme)
            }
        }
    }

    @Test
    fun dotsTrackersAndRingsStandOutFromTheBackground() {
        for (theme in Theme.entries) {
            val p = theme.palette
            check("the tracker color", p.danger, p.bg, 3.0, theme)
            check("the ring's circles", p.outline, p.bg, 3.0, theme)
            for ((kind, color) in p.kinds) check("the $kind dots", color, p.bg, 3.0, theme)
        }
    }

    @Test
    fun lowGlareAndNightKeepEveryDotDimmerThanTheText() {
        for (theme in listOf(Theme.LOW_GLARE, Theme.NIGHT)) {
            val p = theme.palette
            val text = Contrast.ratio(p.text, p.bg)
            for ((kind, color) in p.kinds + ("tracker" to p.danger)) {
                val dot = Contrast.ratio(color, p.bg)
                assertTrue("${theme.label}: the $kind dots (%.1f:1) outshine the text (%.1f:1)".format(dot, text), dot <= text)
            }
        }
    }

    @Test
    fun noThemesChromeLeansBlue() {
        // android.md: color marks data, so no surface or text is navy or blue-purple.
        for (theme in Theme.entries) {
            val p = theme.palette
            val roles = listOf(
                "bg" to p.bg, "surface" to p.surface, "elevated" to p.elevated, "elevatedLight" to p.elevatedLight,
                "outline" to p.outline, "faint" to p.faint, "text" to p.text, "textDim" to p.textDim,
            )
            for ((role, c) in roles) {
                val red = (c shr 16) and 0xff
                val blue = c and 0xff
                assertTrue("${theme.label}: $role %06x leans blue".format(c and 0xffffff), blue <= red)
            }
        }
    }

    @Test
    fun everyKindHasAColorInEveryTheme() {
        val kinds = setOf("ble", "bt_classic", "access_point", "station")
        for (theme in Theme.entries) assertEquals(theme.label, kinds, theme.palette.kinds.keys)
    }

    @Test
    fun anUnknownKeyReadsAsDarkAndTheCycleVisitsEach() {
        assertEquals(Theme.DARK, Theme.of("solarized"))
        assertEquals(Theme.DARK, Theme.of(null))
        assertEquals(Theme.NIGHT, Theme.of("night"))
        assertEquals(listOf(Theme.LOW_GLARE, Theme.NIGHT, Theme.DARK), Theme.entries.map { it.next() })
    }

    @Test
    fun contrastMatchesTheKnownExtremes() {
        assertEquals(21.0, Contrast.ratio(0xFFFFFFFF.toInt(), 0xFF000000.toInt()), 0.01)
        assertEquals(1.0, Contrast.ratio(0xFF777777.toInt(), 0xFF777777.toInt()), 0.01)
    }
}

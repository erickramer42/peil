package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The camera view's geometry: where the camera faces, its field of view, and where a bearing falls across the view. */
class CameraGeometryTest {
    @Test
    fun theCameraFacesAlongTheBackOfThePhone() {
        // Flat, screen up: the camera looks at the floor.
        assertNull(CameraGeometry.facingDeg(floatArrayOf(1f, 0f, 0f, 0f, 1f, 0f, 0f, 0f, 1f)))
        // Upright, screen toward the user, who faces north; then east.
        assertEquals(0.0, CameraGeometry.facingDeg(floatArrayOf(1f, 0f, 0f, 0f, 0f, -1f, 0f, 1f, 0f))!!, 1e-9)
        assertEquals(90.0, CameraGeometry.facingDeg(floatArrayOf(0f, 0f, -1f, -1f, 0f, 0f, 0f, 1f, 0f))!!, 1e-9)
        // Tilted 45 degrees from flat toward north still faces north.
        val c = Math.sqrt(0.5).toFloat()
        assertEquals(0.0, CameraGeometry.facingDeg(floatArrayOf(1f, 0f, 0f, 0f, c, -c, 0f, c, c))!!, 1e-6)
    }

    @Test
    fun aBearingFallsAcrossTheViewByItsTangent() {
        assertEquals(500.0, CameraGeometry.screenX(0.0, 30.0, 1000.0), 1e-9)
        assertEquals(1000.0, CameraGeometry.screenX(30.0, 30.0, 1000.0), 1e-9)
        assertEquals(0.0, CameraGeometry.screenX(-30.0, 30.0, 1000.0), 1e-9)
        assertEquals(1000.0, CameraGeometry.screenX(120.0, 30.0, 1000.0), 1e-9)
        assertEquals(-10.0, CameraGeometry.signed(350.0), 1e-9)
        assertEquals(180.0, CameraGeometry.signed(180.0), 1e-9)
    }

    @Test
    fun aSquareViewShowsTheSensorsShortSideEitherWay() {
        val upright = CameraGeometry.halfFovDeg(6.4, 4.8, 4.8, 1440, 1080, 1000.0, 1000.0, upright = true)
        val turned = CameraGeometry.halfFovDeg(6.4, 4.8, 4.8, 1440, 1080, 1000.0, 1000.0, upright = false)
        assertEquals(26.57, upright, 0.01)
        assertEquals(26.57, turned, 0.01)
        // A 16:9 preview crops the sensor's short side to 3.6 mm.
        assertEquals(20.56, CameraGeometry.halfFovDeg(6.4, 4.8, 4.8, 1920, 1080, 1000.0, 1000.0, upright = true), 0.01)
    }

    @Test
    fun thePreviewKeepsTheSensorsAspectWhereOneDoes() {
        val sizes = listOf(1920 to 1080, 1440 to 1080, 4000 to 3000, 640 to 480)
        assertEquals(1440 to 1080, CameraGeometry.previewSize(sizes, 4.0 / 3.0))
        assertEquals(1920 to 1080, CameraGeometry.previewSize(listOf(1920 to 1080, 1280 to 720), 4.0 / 3.0))
        assertNull(CameraGeometry.previewSize(emptyList(), 4.0 / 3.0))
    }

    @Test
    fun wordsTooWideForTheViewWrapIntoEvenLines() {
        val chars = { s: String -> s.length.toFloat() }
        val hint = "Raise the phone to look through it"
        assertEquals(listOf(hint), CameraGeometry.lines(hint, 40f, chars))
        assertEquals(listOf("Raise the phone to", "look through it"), CameraGeometry.lines(hint, 30f, chars))
        assertEquals(listOf("a", "wordtoolong", "b"), CameraGeometry.lines("a wordtoolong b", 5f, chars))
    }
}

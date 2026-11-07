package io.github.erickramer42.sensio

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.view.View
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin

/**
 * The ring (roadmap item 43): the vehicle at the center, one dot per recent
 * emitter. The ring a dot sits in is its signal-strength band, strongest
 * innermost: strength, never distance. Its angle is its kind's sector, never
 * a direction; nothing here knows where an emitter is. Color is the kind;
 * a tracker is red with a halo.
 */
class RingView(context: Context) : View(context) {
    var picture: Picture? = null
        set(value) {
            field = value
            invalidate()
        }

    private val density = resources.displayMetrics.density
    private val ringPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 1.5f * density
        color = Color.rgb(90, 96, 110)
    }
    private val unknownRingPaint = Paint(ringPaint).apply { pathEffect = DashPathEffect(floatArrayOf(6 * density, 6 * density), 0f) }
    private val sectorPaint = Paint(ringPaint).apply { color = Color.rgb(55, 60, 70) }
    private val dot = Paint(Paint.ANTI_ALIAS_FLAG)
    private val halo = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 2.5f * density
        color = TRACKER
    }
    private val label = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.rgb(170, 176, 190)
        textSize = 12 * density
    }

    private val bandLabel = Paint(label).apply {
        color = Color.rgb(120, 126, 140)
        textSize = 10 * density
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val w = MeasureSpec.getSize(widthMeasureSpec)
        val h = MeasureSpec.getSize(heightMeasureSpec)
        val side = if (h == 0) w else min(w, h)
        setMeasuredDimension(side, side)
    }

    override fun onDraw(canvas: Canvas) {
        val cx = width / 2f
        val cy = height / 2f
        val r = min(cx, cy) - 8 * density

        for (sector in SECTORS) {
            val a = Math.toRadians(sector.from.toDouble())
            canvas.drawLine(cx, cy, cx + r * cos(a).toFloat(), cy + r * sin(a).toFloat(), sectorPaint)
        }
        for ((band, outer) in BANDS) {
            canvas.drawCircle(cx, cy, outer * r, if (band == "unknown") unknownRingPaint else ringPaint)
        }
        for ((kind, from, span) in SECTORS) {
            val a = Math.toRadians((from + span / 2).toDouble())
            val text = KIND_LABELS.getValue(kind)
            val tx = cx + 0.9f * r * cos(a).toFloat() - label.measureText(text) / 2
            val ty = cy + 0.9f * r * sin(a).toFloat()
            canvas.drawText(text, tx, ty, label)
        }
        // Each band's name at its ring's right edge, just above the horizon; the
        // outermost one below it, where it has room.
        for ((band, outer) in BANDS) {
            val text = BAND_LABELS.getValue(band)
            val y = if (band == "unknown") cy + bandLabel.textSize + 2 * density else cy - 4 * density
            canvas.drawText(text, cx + outer * r - bandLabel.measureText(text) - 4 * density, y, bandLabel)
        }
        dot.color = Color.WHITE
        canvas.drawCircle(cx, cy, 5 * density, dot)

        val emitters = picture?.emitters ?: return
        for (e in emitters) {
            val (from, span) = SECTORS.firstOrNull { it.kind == e.kind }?.let { it.from to it.span } ?: continue
            val bandIndex = BANDS.indexOfFirst { it.first == e.band }.takeIf { it >= 0 } ?: (BANDS.size - 1)
            val inner = if (bandIndex == 0) 0.08f else BANDS[bandIndex - 1].second
            val outer = BANDS[bandIndex].second
            // A stable spot for each device within its band and sector.
            val h = e.device.hashCode()
            val fa = ((h ushr 8) and 0xffff) / 65535f
            val fr = (h and 0xff) / 255f
            val angle = Math.toRadians((from + span * (0.08f + 0.84f * fa)).toDouble())
            val radius = r * (inner + (outer - inner) * (0.2f + 0.6f * fr))
            val x = cx + radius * cos(angle).toFloat()
            val y = cy + radius * sin(angle).toFloat()
            dot.color = if (e.tracker != null) TRACKER else KIND_COLORS[e.kind] ?: Color.GRAY
            canvas.drawCircle(x, y, (if (e.tracker != null) 6f else 4.5f) * density, dot)
            if (e.tracker != null) canvas.drawCircle(x, y, 11 * density, halo)
        }
    }

    private data class Sector(val kind: String, val from: Float, val span: Float)

    companion object {
        /** Each band's outer edge, as a fraction of the radius, strongest first. */
        private val BANDS = listOf("strong" to 0.32f, "medium" to 0.56f, "weak" to 0.8f, "unknown" to 1f)

        /** Degrees clockwise from east: BLE across the top, access points lower right, stations and TPMS lower left. */
        private val SECTORS = listOf(
            Sector("ble", 180f, 180f),
            Sector("access_point", 0f, 110f),
            Sector("station", 110f, 40f),
            Sector("tpms", 150f, 30f),
        )

        private val BAND_LABELS = mapOf("strong" to ">= -60", "medium" to "-60..-80", "weak" to "< -80", "unknown" to "no dBm")

        private val KIND_LABELS =mapOf("ble" to "BLE", "access_point" to "Wi-Fi AP", "station" to "Station", "tpms" to "TPMS")

        private val KIND_COLORS = mapOf(
            "ble" to Color.rgb(80, 200, 120),
            "access_point" to Color.rgb(90, 160, 255),
            "station" to Color.rgb(80, 220, 230),
            "tpms" to Color.rgb(240, 180, 60),
        )

        val TRACKER: Int = Color.rgb(255, 80, 80)
    }
}

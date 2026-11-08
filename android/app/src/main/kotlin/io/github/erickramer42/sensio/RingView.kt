package io.github.erickramer42.sensio

import android.content.Context
import android.graphics.Canvas
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.view.MotionEvent
import android.view.View
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin

/**
 * The ring (roadmap item 43): the vehicle at the center, one dot per recent
 * emitter. The ring a dot sits in is its signal-strength band, strongest
 * innermost: strength, never distance. Its angle is its kind's sector, never
 * a direction; nothing here knows where an emitter is. Color is the kind;
 * a tracker takes the theme's danger color and a halo, a shape as well as a
 * color, since in Night nearly everything is red. A tap on a dot selects it,
 * and the selected dot gets a ring in the text color; [onTap] hears the tapped
 * device, or null for a tap away from every dot. Colors come from [palette].
 */
class RingView(context: Context, private val palette: Palette) : View(context) {
    var picture: Picture? = null
        set(value) {
            field = value
            invalidate()
        }

    var selected: String? = null
        set(value) {
            field = value
            invalidate()
        }

    var onTap: ((String?) -> Unit)? = null

    /** Where each dot was last drawn, for taps. */
    private var spots: List<Spot> = emptyList()

    private val density = resources.displayMetrics.density
    private val ringPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 1.5f * density
        color = palette.outline
    }
    private val unknownRingPaint = Paint(ringPaint).apply { pathEffect = DashPathEffect(floatArrayOf(6 * density, 6 * density), 0f) }
    private val sectorPaint = Paint(ringPaint).apply { color = palette.faint }
    private val dot = Paint(Paint.ANTI_ALIAS_FLAG)
    private val halo = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 2.5f * density
        color = palette.danger
    }
    private val selectedRing = Paint(halo).apply { color = palette.text }
    private val label = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = palette.text
        textSize = 12 * density
    }

    private val bandLabel = Paint(label).apply {
        color = palette.textDim
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

        val sectors = sectors(picture?.emitters.orEmpty().mapTo(HashSet()) { it.kind })
        if (sectors.size > 1) {
            for (sector in sectors) {
                val a = Math.toRadians(sector.from.toDouble())
                canvas.drawLine(cx, cy, cx + r * cos(a).toFloat(), cy + r * sin(a).toFloat(), sectorPaint)
            }
        }
        for ((band, outer) in BANDS) {
            canvas.drawCircle(cx, cy, outer * r, if (band == "unknown") unknownRingPaint else ringPaint)
        }
        for ((kind, from, span) in sectors) {
            val a = Math.toRadians((from + span / 2).toDouble())
            val text = KIND_LABELS[kind] ?: kind
            val tx = cx + 0.9f * r * cos(a).toFloat() - label.measureText(text) / 2
            val ty = cy + 0.9f * r * sin(a).toFloat()
            canvas.drawText(text, tx, ty, label)
        }
        // Each band's name at its ring's right edge, just above the horizon; the
        // outermost one below it, where it has room. A small ring leaves out the
        // labels its bands are too narrow for.
        for ((i, entry) in BANDS.withIndex()) {
            val (band, outer) = entry
            val text = BAND_LABELS.getValue(band)
            val room = (outer - (if (i == 0) 0f else BANDS[i - 1].second)) * r
            if (band != "unknown" && bandLabel.measureText(text) + 8 * density > room) continue
            val y = if (band == "unknown") cy + bandLabel.textSize + 2 * density else cy - 4 * density
            canvas.drawText(text, cx + outer * r - bandLabel.measureText(text) - 4 * density, y, bandLabel)
        }
        dot.color = palette.text
        canvas.drawCircle(cx, cy, 5 * density, dot)

        val emitters = picture?.emitters ?: return
        val drawn = ArrayList<Spot>(emitters.size)
        for (e in emitters) {
            val (from, span) = sectors.firstOrNull { it.kind == e.kind }?.let { it.from to it.span } ?: continue
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
            dot.color = if (e.tracker != null) palette.danger else palette.kinds[e.kind] ?: palette.textDim
            canvas.drawCircle(x, y, (if (e.tracker != null) 6f else 4.5f) * density, dot)
            if (e.tracker != null) canvas.drawCircle(x, y, 11 * density, halo)
            if (e.device == selected) canvas.drawCircle(x, y, 15 * density, selectedRing)
            drawn.add(Spot(e.device, x, y))
        }
        spots = drawn
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> return true
            MotionEvent.ACTION_UP -> {
                // A fingertip is about 10 mm across; dots are a few millimeters apart.
                onTap?.invoke(Selection.nearest(spots, event.x, event.y, 20 * density))
                performClick()
                return true
            }
        }
        return super.onTouchEvent(event)
    }

    override fun performClick(): Boolean = super.performClick()

    private data class Sector(val kind: String, val from: Float, val span: Float)

    /**
     * A slice for each kind with something in it, none for the others, sized
     * by [WEIGHTS] and laid clockwise from west in [WEIGHTS]' order.
     */
    private fun sectors(present: Set<String>): List<Sector> {
        val kinds = WEIGHTS.filter { it.first in present }
        val total = kinds.sumOf { it.second.toDouble() }.toFloat()
        var from = 180f
        return kinds.map { (kind, weight) ->
            val span = 360f * weight / total
            Sector(kind, from, span).also { from = (from + span) % 360f }
        }
    }

    companion object {
        /** Each band's outer edge, as a fraction of the radius, strongest first. */
        private val BANDS = listOf("strong" to 0.32f, "medium" to 0.56f, "weak" to 0.8f, "unknown" to 1f)

        /** Each kind's share of the circle when all are present; BLE first, across the top. */
        private val WEIGHTS = listOf(
            "ble" to 180f,
            "bt_classic" to 40f,
            "access_point" to 110f,
            "station" to 40f,
            "tpms" to 30f,
        )

        private val BAND_LABELS = mapOf("strong" to ">= -60", "medium" to "-60..-80", "weak" to "< -80", "unknown" to "no dBm")

        private val KIND_LABELS = mapOf(
            "ble" to "BLE",
            "bt_classic" to "BT Classic",
            "access_point" to "Wi-Fi AP",
            "station" to "Station",
            "tpms" to "TPMS",
        )
    }
}

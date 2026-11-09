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
 * emitter. How far out a dot sits is its strongest recent signal on one scale
 * ([RingScale]), with circles at the core's band edges: strength, never
 * distance. Its angle is its kind's sector, never a direction; nothing here
 * knows where an emitter is. Color is the kind;
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
        val rim = picture?.emitters.orEmpty().any { RingScale.onRim(it) }
        for (dbm in RingScale.GUIDES) canvas.drawCircle(cx, cy, RingScale.fraction(dbm, rim) * r, ringPaint)
        if (rim) canvas.drawCircle(cx, cy, r, unknownRingPaint)
        for ((kind, from, span) in sectors) {
            val a = Math.toRadians((from + span / 2).toDouble())
            val text = KIND_LABELS[kind] ?: kind
            val tx = cx + 0.9f * r * cos(a).toFloat() - label.measureText(text) / 2
            val ty = cy + 0.9f * r * sin(a).toFloat()
            canvas.drawText(text, tx, ty, label)
        }
        // Each circle's strength just inside its right edge, above the horizon,
        // and the rim's "no dBm" below it when the rim is drawn. A small ring
        // leaves out a label with no room before the circle inside it.
        var previous = RingScale.INNER * r
        for (dbm in RingScale.GUIDES) {
            val edge = RingScale.fraction(dbm, rim) * r
            val text = "$dbm"
            if (bandLabel.measureText(text) + 8 * density <= edge - previous) {
                canvas.drawText(text, cx + edge - bandLabel.measureText(text) - 4 * density, cy - 4 * density, bandLabel)
            }
            previous = edge
        }
        if (rim) {
            val text = "no dBm"
            canvas.drawText(text, cx + r - bandLabel.measureText(text) - 4 * density, cy + bandLabel.textSize + 2 * density, bandLabel)
        }
        dot.color = palette.text
        canvas.drawCircle(cx, cy, 5 * density, dot)

        val emitters = picture?.emitters ?: return
        val drawn = ArrayList<Spot>(emitters.size)
        for (e in emitters) {
            val (from, span) = sectors.firstOrNull { it.kind == e.kind }?.let { it.from to it.span } ?: continue
            // A stable angle for each device within its sector; on the rim, a
            // stable spot across it too, since there's no strength to place it by.
            val h = e.device.hashCode()
            val fa = ((h ushr 8) and 0xffff) / 65535f
            val fr = (h and 0xff) / 255f
            val angle = Math.toRadians((from + span * (0.08f + 0.84f * fa)).toDouble())
            val radius = r * RingScale.place(e, rim, fr)
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
        /** Each kind's share of the circle when all are present; BLE first, across the top. */
        private val WEIGHTS = listOf(
            "ble" to 180f,
            "bt_classic" to 40f,
            "access_point" to 110f,
            "station" to 40f,
            "tpms" to 30f,
        )

        private val KIND_LABELS = mapOf(
            "ble" to "BLE",
            "bt_classic" to "BT Classic",
            "access_point" to "Wi-Fi AP",
            "station" to "Station",
            "tpms" to "TPMS",
        )
    }
}

/**
 * Where a dot sits, as a fraction of the ring's radius: its strongest recent
 * strength on one linear scale, [STRONGEST] dBm just outside the center dot
 * to [WEAKEST] at the edge, stronger and weaker ones held to the ends. Most
 * emitters on a drive are below -80 dBm, and a linear scale gives that band
 * the outer third of the radius, more than half the ring's area. An emitter
 * with no strength (a TPMS sensor, a Kismet record with none) goes on a rim
 * outside [WEAKEST], drawn only while something is on it.
 */
object RingScale {
    const val STRONGEST = -40
    const val WEAKEST = -100

    /** The circles drawn: the core's band edges (DescribeParams' defaults) and the scale's end. */
    val GUIDES = listOf(-60, -80, WEAKEST)

    /** Clear of the center dot. */
    const val INNER = 0.08f

    /** Where the scale ends when the rim is drawn; the rim is the rest. */
    const val RIM = 0.9f

    fun onRim(e: Emitter): Boolean = e.band == "unknown" || e.strongestDbm == null

    /** A strength's place, with the rim drawn or not. */
    fun fraction(dbm: Number, rim: Boolean): Float {
        val outer = if (rim) RIM else 1f
        val t = ((STRONGEST - dbm.toFloat()) / (STRONGEST - WEAKEST)).coerceIn(0f, 1f)
        return INNER + (outer - INNER) * t
    }

    /** An emitter's place; [spread] (0 to 1, stable per device) places it across the rim. */
    fun place(e: Emitter, rim: Boolean, spread: Float): Float {
        val dbm = e.strongestDbm
        return if (dbm == null || onRim(e)) RIM + (1f - RIM) * (0.25f + 0.5f * spread) else fraction(dbm, rim)
    }
}

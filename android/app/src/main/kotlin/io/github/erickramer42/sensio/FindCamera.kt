package io.github.erickramer42.sensio

import android.content.Context
import android.graphics.Canvas
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.SurfaceTexture
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.params.OutputConfiguration
import android.hardware.camera2.params.SessionConfiguration
import android.os.Handler
import android.os.HandlerThread
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.widget.FrameLayout
import kotlin.math.abs
import kotlin.math.atan
import kotlin.math.atan2
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.tan

/**
 * Find mode's camera view (roadmap item 39): the back camera's live preview
 * with the core's bearing drawn over it as a band, where the sweep heard the
 * target strongest, and an arrow at the edge when that band is out of view.
 *
 * The preview goes to the screen's surface and nowhere else: no frame is
 * captured, read, analyzed, stored or sent (ADR-0002 section 5, ADR-0004
 * section 7). The view reads the rotation-vector sensor itself, for the
 * direction the camera faces, and uses it for drawing only; the sweep pairs
 * readings with the session's headings (SessionService), so the feed is the
 * same whether the view is on or not.
 */
class FindCamera(context: Context, private val palette: Palette) : FrameLayout(context) {
    /** The core's latest answer for the target, as the ring has it. */
    var answer: FindAnswer? = null
        set(value) {
            field = value
            overlay.invalidate()
        }

    /** The target as last heard. */
    var target: Emitter? = null
        set(value) {
            field = value
            overlay.invalidate()
        }

    /** Why there's no preview, when there isn't one. */
    private var problem: String? = null

    /** Degrees from magnetic north the camera faces, or null when it faces the floor or the sky. */
    private var facingDeg: Double? = null
    private var halfFovDeg = 30.0
    private var preview: Pair<Int, Int>? = null

    /** The sensor's long and short sides and the focal length, millimeters. */
    private var optics: Triple<Double, Double, Double>? = null
    private var run: Run? = null

    private val texture = TextureView(context)
    private val overlay = Overlay(context)
    private val sensors = context.getSystemService(SensorManager::class.java)
    private val rotation = FloatArray(9)

    private val headingListener = object : SensorEventListener {
        override fun onSensorChanged(event: SensorEvent) {
            SensorManager.getRotationMatrixFromVector(rotation, event.values)
            facingDeg = CameraGeometry.facingDeg(rotation)
            overlay.invalidate()
        }

        override fun onAccuracyChanged(sensor: Sensor, accuracy: Int) = Unit
    }

    init {
        setBackgroundColor(palette.bg)
        addView(texture, LayoutParams(-1, -1))
        addView(overlay, LayoutParams(-1, -1))
        texture.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
            override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                transform()
                run?.surface(st)
            }

            override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) = transform()

            override fun onSurfaceTextureDestroyed(st: SurfaceTexture): Boolean {
                stop()
                return true
            }

            override fun onSurfaceTextureUpdated(st: SurfaceTexture) = Unit
        }
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val w = MeasureSpec.getSize(widthMeasureSpec)
        val h = MeasureSpec.getSize(heightMeasureSpec)
        val side = if (h == 0) w else min(w, h)
        val exact = MeasureSpec.makeMeasureSpec(side, MeasureSpec.EXACTLY)
        super.onMeasure(exact, exact)
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        transform()
    }

    /** Opens the back camera and the heading; the caller has the camera permission. */
    fun start() {
        if (run != null) return
        problem = null
        val manager = context.getSystemService(CameraManager::class.java)
        val id = runCatching {
            manager.cameraIdList.firstOrNull {
                manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK
            }
        }.getOrNull()
        if (id == null) {
            problem = "No back camera"
            overlay.invalidate()
            return
        }
        val chars = runCatching { manager.getCameraCharacteristics(id) }.getOrElse {
            problem = "The camera wouldn't open"
            overlay.invalidate()
            return
        }
        val active = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE)
        val sizes = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?.getOutputSizes(SurfaceTexture::class.java).orEmpty().map { it.width to it.height }
        val chosen = CameraGeometry.previewSize(sizes, active?.let { it.width().toDouble() / it.height() } ?: (4.0 / 3.0))
        if (chosen == null) {
            problem = "The camera offers no preview size"
            overlay.invalidate()
            return
        }
        preview = chosen
        val physical = chars.get(CameraCharacteristics.SENSOR_INFO_PHYSICAL_SIZE)
        val focal = chars.get(CameraCharacteristics.LENS_INFO_AVAILABLE_FOCAL_LENGTHS)?.firstOrNull()
        val optics = if (physical != null && focal != null && focal > 0f) {
            Triple(max(physical.width, physical.height).toDouble(), min(physical.width, physical.height).toDouble(), focal.toDouble())
        } else {
            null
        }
        this.optics = optics
        transform()
        run = Run(manager, id, chosen).also { r -> if (texture.isAvailable) texture.surfaceTexture?.let(r::surface) }
        sensors?.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR)?.let {
            sensors.registerListener(headingListener, it, SensorManager.SENSOR_DELAY_UI)
        }
    }

    /** Closes the camera and the heading. */
    fun stop() {
        sensors?.unregisterListener(headingListener)
        run?.close()
        run = null
        facingDeg = null
    }

    /** Fills the square with the preview's middle, upright, and works out the field of view that leaves across it. */
    private fun transform() {
        val (pLong, pShort) = preview ?: return
        val vw = width.toFloat()
        val vh = height.toFloat()
        if (vw <= 0f || vh <= 0f) return
        val turn = display?.rotation ?: Surface.ROTATION_0
        val upright = turn == Surface.ROTATION_0 || turn == Surface.ROTATION_180
        val cx = vw / 2
        val cy = vh / 2
        val matrix = Matrix()
        if (upright) {
            // The buffer already arrives upright, stretched over the view.
            val s = max(vw / pShort, vh / pLong)
            matrix.setScale(pShort * s / vw, pLong * s / vh, cx, cy)
            if (turn == Surface.ROTATION_180) matrix.postRotate(180f, cx, cy)
        } else {
            val view = RectF(0f, 0f, vw, vh)
            val buffer = RectF(0f, 0f, pShort.toFloat(), pLong.toFloat())
            buffer.offset(cx - buffer.centerX(), cy - buffer.centerY())
            matrix.setRectToRect(view, buffer, Matrix.ScaleToFit.FILL)
            val s = max(vh / pShort, vw / pLong)
            matrix.postScale(s, s, cx, cy)
            matrix.postRotate(90f * (turn - 2), cx, cy)
        }
        texture.setTransform(matrix)
        optics?.let { (long, short, focal) ->
            halfFovDeg = CameraGeometry.halfFovDeg(long, short, focal, pLong, pShort, vw.toDouble(), vh.toDouble(), upright)
        }
        overlay.invalidate()
    }

    /**
     * One opening of the camera, on its own thread. A close before the camera
     * has opened waits for it to open, then closes it, so a camera is never
     * left open on a thread that has quit.
     */
    private inner class Run(manager: CameraManager, id: String, val size: Pair<Int, Int>) {
        private val thread = HandlerThread("camera").apply { start() }
        private val handler = Handler(thread.looper)
        private var camera: CameraDevice? = null
        private var session: CameraCaptureSession? = null
        private var surfaceTexture: SurfaceTexture? = null
        private var pending = true
        private var closing = false

        private val callback = object : CameraDevice.StateCallback() {
            override fun onOpened(cam: CameraDevice) {
                pending = false
                if (closing) {
                    cam.close()
                    thread.quitSafely()
                    return
                }
                camera = cam
                begin()
            }

            override fun onDisconnected(cam: CameraDevice) = ended(cam, "Another app took the camera")

            override fun onError(cam: CameraDevice, error: Int) = ended(cam, "The camera stopped (error $error)")
        }

        // After callback: the camera thread can run this post before the constructor reaches a later initializer.
        init {
            handler.post {
                try {
                    manager.openCamera(id, callback, handler)
                } catch (e: Exception) {
                    pending = false
                    report("The camera wouldn't open")
                    if (closing) thread.quitSafely()
                }
            }
        }

        private fun ended(cam: CameraDevice, why: String) {
            pending = false
            cam.close()
            camera = null
            if (closing) thread.quitSafely() else report(why)
        }

        fun surface(st: SurfaceTexture) {
            handler.post {
                surfaceTexture = st
                begin()
            }
        }

        /** The preview's repeating request, once both the camera and the surface are there. */
        private fun begin() {
            val cam = camera ?: return
            val st = surfaceTexture ?: return
            if (session != null) return
            st.setDefaultBufferSize(size.first, size.second)
            val surface = Surface(st)
            val config = SessionConfiguration(
                SessionConfiguration.SESSION_REGULAR,
                listOf(OutputConfiguration(surface)),
                { handler.post(it) },
                object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(s: CameraCaptureSession) {
                        if (closing) return s.close()
                        session = s
                        runCatching {
                            val request = cam.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply { addTarget(surface) }
                            s.setRepeatingRequest(request.build(), null, handler)
                        }.onFailure { report("The preview wouldn't start") }
                    }

                    override fun onConfigureFailed(s: CameraCaptureSession) = report("The preview wouldn't start")
                },
            )
            runCatching { cam.createCaptureSession(config) }.onFailure { report("The preview wouldn't start") }
        }

        fun close() {
            handler.post {
                closing = true
                runCatching { session?.close() }
                session = null
                runCatching { camera?.close() }
                camera = null
                if (!pending) thread.quitSafely()
            }
        }

        private fun report(why: String) {
            post {
                problem = why
                overlay.invalidate()
            }
        }
    }

    /** The band, the arrow and the words, over the preview. */
    private inner class Overlay(context: Context) : View(context) {
        private val density = resources.displayMetrics.density
        private val band = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.FILL
            color = palette.text
            alpha = 48
        }
        private val edge = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = 1.5f * density
            color = palette.text
        }
        private val arrow = Paint(edge).apply { strokeWidth = 3 * density }
        private val label = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            typeface = Ui.sans(context)
            color = palette.text
            textSize = 14 * density
        }
        private val outline = Paint(label).apply {
            style = Paint.Style.STROKE
            strokeWidth = 3 * density
            strokeJoin = Paint.Join.ROUND
            color = palette.bg
        }
        private val chevron = Path()

        override fun onDraw(canvas: Canvas) {
            val w = width.toFloat()
            val h = height.toFloat()
            val bottom = h - 12 * density
            problem?.let { return words(canvas, it, w / 2, bottom) }
            val facing = facingDeg ?: return words(canvas, "Raise the phone to look through it", w / 2, bottom)
            val answer = answer
            val bearing = answer?.bearingDeg
            if (answer == null || bearing == null || answer.covered.isEmpty()) {
                return words(canvas, "No direction yet: turn slowly", w / 2, bottom)
            }
            val span = 360.0 / answer.covered.size
            val rel = CameraGeometry.signed(Find.relative(bearing, facing))
            val heard = target?.latestDbm?.let { "%.0f dBm".format(it) }
            when {
                rel + span / 2 < -halfFovDeg -> turn(canvas, left = true, degrees = -rel, w, h)
                rel - span / 2 > halfFovDeg -> turn(canvas, left = false, degrees = rel, w, h)
                else -> {
                    val x1 = CameraGeometry.screenX(rel - span / 2, halfFovDeg, w.toDouble()).toFloat()
                    val x2 = CameraGeometry.screenX(rel + span / 2, halfFovDeg, w.toDouble()).toFloat()
                    canvas.drawRect(x1, 0f, x2, h, band)
                    if (x1 > 0f) canvas.drawLine(x1, 0f, x1, h, edge)
                    if (x2 < w) canvas.drawLine(x2, 0f, x2, h, edge)
                    words(canvas, listOfNotNull("Strongest here", heard).joinToString(", "), (x1 + x2) / 2, bottom)
                }
            }
        }

        private fun turn(canvas: Canvas, left: Boolean, degrees: Double, w: Float, h: Float) {
            val size = 18 * density
            val tip = if (left) 16 * density else w - 16 * density
            val back = if (left) tip + size else tip - size
            chevron.reset()
            chevron.moveTo(back, h / 2 - size)
            chevron.lineTo(tip, h / 2)
            chevron.lineTo(back, h / 2 + size)
            canvas.drawPath(chevron, arrow)
            val text = "Turn ${if (left) "left" else "right"} ${abs(degrees).roundToInt()}°"
            words(canvas, text, w / 2, h - 12 * density)
        }

        /** [text] centered on [x] with its last line on [y], wrapped to the view's width and kept inside it, outlined so it reads over the preview. */
        private fun words(canvas: Canvas, text: String, x: Float, y: Float) {
            val pad = 8 * density
            val lines = CameraGeometry.lines(text, width - 2 * pad) { label.measureText(it) }
            lines.forEachIndexed { i, line ->
                val half = label.measureText(line) / 2
                val at = x.coerceIn(half + pad, max(half + pad, width - half - pad)) - half
                val baseline = y - (lines.size - 1 - i) * label.fontSpacing
                canvas.drawText(line, at, baseline, outline)
                canvas.drawText(line, at, baseline, label)
            }
        }
    }
}

/** The camera view's geometry, apart from the views, so it can be tested. */
object CameraGeometry {
    /** Within this many degrees of straight down or up, the camera's direction is too uncertain to draw by. */
    const val STEEPEST_DEG = 25.0

    /**
     * The direction the back camera faces, degrees clockwise from the
     * rotation vector's north (0 to under 360), from its rotation matrix
     * (device to world, row-major, as `SensorManager.getRotationMatrixFromVector`
     * gives it); null when the camera points within [STEEPEST_DEG] of the
     * floor or the sky. The camera looks along the device's -z axis.
     */
    fun facingDeg(r: FloatArray): Double? {
        val east = -r[2].toDouble()
        val north = -r[5].toDouble()
        if (hypot(east, north) < kotlin.math.sin(Math.toRadians(STEEPEST_DEG))) return null
        val deg = Math.toDegrees(atan2(east, north))
        return if (deg < 0) deg + 360.0 else deg
    }

    /** A bearing from 0 to under 360 as -180 to 180, negative to the left. */
    fun signed(deg: Double): Double = if (deg > 180.0) deg - 360.0 else deg

    /** Where [relDeg] from the camera's axis falls across a view [width] wide, a pinhole camera of [halfFovDeg]. */
    fun screenX(relDeg: Double, halfFovDeg: Double, width: Double): Double {
        val clamped = relDeg.coerceIn(-89.0, 89.0)
        val x = width / 2 + tan(Math.toRadians(clamped)) / tan(Math.toRadians(halfFovDeg)) * width / 2
        return x.coerceIn(0.0, width)
    }

    /**
     * Half the horizontal field of view the view shows, degrees: the sensor
     * ([sensorLong] by [sensorShort] mm, focal length [focal] mm), cropped to
     * the preview's aspect ([previewLong] by [previewShort]), then to the
     * view's ([viewW] by [viewH]); [upright] when the screen is in portrait,
     * so the view's width lies along the sensor's short side.
     */
    fun halfFovDeg(
        sensorLong: Double, sensorShort: Double, focal: Double,
        previewLong: Int, previewShort: Int, viewW: Double, viewH: Double, upright: Boolean,
    ): Double {
        val aspect = previewLong.toDouble() / previewShort
        val (usedLong, usedShort) =
            if (aspect >= sensorLong / sensorShort) sensorLong to sensorLong / aspect else sensorShort * aspect to sensorShort
        val (cw, ch, across) =
            if (upright) Triple(previewShort.toDouble(), previewLong.toDouble(), usedShort)
            else Triple(previewLong.toDouble(), previewShort.toDouble(), usedLong)
        val scale = max(viewW / cw, viewH / ch)
        val shown = viewW / (cw * scale)
        return Math.toDegrees(atan(shown * across / 2 / focal))
    }

    /**
     * The preview size to ask for: the largest no longer than 1920 pixels
     * with the sensor's [aspect] (within 2%), so no part of the sensor is
     * cropped away; else the largest no longer than 1920; null for none.
     */
    fun previewSize(sizes: List<Pair<Int, Int>>, aspect: Double): Pair<Int, Int>? {
        val landscape = sizes.map { (a, b) -> max(a, b) to min(a, b) }.filter { it.second > 0 && it.first <= 1920 }
        val matching = landscape.filter { abs(it.first.toDouble() / it.second / aspect - 1) <= 0.02 }
        return (matching.ifEmpty { landscape }).maxByOrNull { it.first.toLong() * it.second }
    }

    /**
     * [text] broken at spaces into the fewest lines no wider than [room] by
     * [measure], their widths as even as that allows; a word wider than
     * [room] has a line to itself.
     */
    fun lines(text: String, room: Float, measure: (String) -> Float): List<String> {
        val fewest = greedy(text, room, measure)
        var narrow = max(measure(text) / max(fewest.size, 1), 1f)
        while (narrow < room) {
            val even = greedy(text, narrow, measure)
            if (even.size <= fewest.size) return even
            narrow *= 1.05f
        }
        return fewest
    }

    private fun greedy(text: String, room: Float, measure: (String) -> Float): List<String> {
        val lines = mutableListOf<String>()
        var line = ""
        for (word in text.split(' ').filter { it.isNotEmpty() }) {
            val next = if (line.isEmpty()) word else "$line $word"
            if (line.isNotEmpty() && measure(next) > room) {
                lines += line
                line = word
            } else {
                line = next
            }
        }
        if (line.isNotEmpty()) lines += line
        return lines
    }
}

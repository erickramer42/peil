package io.github.erickramer42.sensio

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * The screen: the ring, one row of controls, and below them, scrolling
 * together, the session's status and coverage and the loudest emitters as a
 * list. A tap on a dot selects that emitter: its card comes first, and the
 * list puts the emitters closest to it in signal strength first. A tap away
 * from the dots clears the selection, or with none, switches the ring between
 * large and small. The settings sit behind one button. The screen stays on
 * while a session runs (roadmap item 69): the driver keeps the full picture,
 * and BLE keeps its match-everything scan.
 */
class MainActivity : Activity() {
    private lateinit var ring: RingView
    private lateinit var body: LinearLayout
    private lateinit var scroll: ScrollView
    /** The emitter the body is laid out for, so it's rearranged only when that changes. */
    private var laidOutFor: String? = ""
    private lateinit var card: TextView
    private lateinit var status: TextView
    private lateinit var list: TextView
    private lateinit var startStop: Button
    private var selected: String? = null
    private val refresh: () -> Unit = { show() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
        val pad = (12 * resources.displayMetrics.density).toInt()

        ring = RingView(this).apply { onTap = ::tapped }
        card = text(13f).apply { typeface = Typeface.MONOSPACE }
        status = text(13f)
        list = text(12f).apply { typeface = Typeface.MONOSPACE }
        startStop = button("Start") { if (Live.running) stop() else start() }

        val buttons = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            // Labels of different sizes would otherwise shift their buttons down to one baseline.
            isBaselineAligned = false
            listOf(startStop, button("Mine") { chooseMine() }, button("Settings") { settings() })
                .forEach { addView(it, LinearLayout.LayoutParams(0, -2, 1f)) }
        }
        body = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val side = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(buttons)
            scroll = ScrollView(this@MainActivity).apply { addView(body) }
            addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        }
        val root = LinearLayout(this).apply {
            orientation = if (landscape) LinearLayout.HORIZONTAL else LinearLayout.VERTICAL
            setBackgroundColor(Color.rgb(18, 20, 24))
            setPadding(pad, pad, pad, pad)
            addView(ring)
            addView(side, if (landscape) LinearLayout.LayoutParams(0, -1, 1f) else LinearLayout.LayoutParams(-1, 0, 1f))
        }
        placeRing()
        // Android 15 and later draw edge to edge: pad by the system bars and the cutout.
        root.setOnApplyWindowInsetsListener { v, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            v.setPadding(bars.left + pad, bars.top + pad, bars.right + pad, bars.bottom + pad)
            insets
        }
        setContentView(root)
    }

    override fun onStart() {
        super.onStart()
        Live.listen(refresh)
        show()
    }

    override fun onStop() {
        Live.unlisten(refresh)
        super.onStop()
    }

    private fun text(size: Float) = TextView(this).apply {
        textSize = size
        setTextColor(Color.rgb(220, 224, 232))
        setPadding(0, 8, 0, 8)
    }

    private fun button(label: String, action: () -> Unit) = Button(this).apply {
        text = label
        isAllCaps = false
        // One line in every row: a long label shrinks rather than wrapping.
        maxLines = 1
        setAutoSizeTextTypeUniformWithConfiguration(10, 16, 1, TypedValue.COMPLEX_UNIT_SP)
        setOnClickListener { action() }
    }

    private fun show() {
        val running = Live.running
        if (running) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        startStop.text = if (running) "Stop" else "Start"

        val enrolled = Enrolled.devices(this)
        val all = Live.picture
        val picture = all?.let { p ->
            Picture(p.movement, p.emitters.filter { it.device !in enrolled }, p.older, p.recentS, p.atNs, p.recentNs, p.texture)
        }
        val hidden = (all?.emitters?.size ?: 0) - (picture?.emitters?.size ?: 0)
        ring.picture = picture
        val chosen = picture?.emitters?.firstOrNull { it.device == selected }
        // An emitter that left the picture can't be shown or compared.
        if (chosen == null) selected = null
        ring.selected = selected
        val coverage = Live.coverage
        status.text = buildString {
            append(Live.status)
            Live.link?.let { append('\n').append(it) }
            if (picture != null) {
                append("\n${picture.emitters.size} emitters in the last ${picture.recentS} s, ${picture.older} older; ${picture.movement}")
                if (hidden > 0) append("; $hidden of yours not shown")
            }
            all?.let { TextureText.lines(it, enrolled).forEach { line -> append('\n').append(line) } }
            if (coverage != null) {
                CoverageText.lines(coverage).forEach { append('\n').append(it) }
                append("\n${coverage.lines} lines, ${coverage.events} events, ${coverage.rejected} rejected")
                if (coverage.withheld + coverage.dropped > 0) append(", ${coverage.dropped} not passed to the core")
                if (coverage.late > 0) append(", ${coverage.late} over 10 min late")
                if (coverage.sleptS > 0) append(", asleep ${coverage.sleptS} s")
            }
            append("\nRings: signal strength, not distance. Angle: kind, not direction.")
        }
        val emitters = picture?.emitters.orEmpty()
        if (chosen != null) {
            card.text = Selection.card(chosen, picture.atNs).joinToString("\n")
            list.text = "Closest in signal strength (strength, not location):\n\n" +
                Selection.similar(emitters, chosen).take(LIST_LENGTH).joinToString("\n") { line(it) }
        } else {
            list.text = emitters.take(LIST_LENGTH).joinToString("\n") { line(it) }
        }
        // With a selection, its card and the list come first; otherwise the status does.
        // Rearranged, and scrolled to the top, only when the selection changes, so
        // an update every few seconds keeps the user's scroll position.
        if (laidOutFor != selected) {
            laidOutFor = selected
            body.removeAllViews()
            (if (chosen != null) listOf(card, list, status) else listOf(status, list)).forEach(body::addView)
            scroll.scrollTo(0, 0)
        }
    }

    /** A tap on the ring: a dot selects it; elsewhere clears the selection, or with none, resizes the ring. */
    private fun tapped(device: String?) {
        when {
            device != null -> selected = if (device == selected) null else device
            selected != null -> selected = null
            else -> {
                Options.setRingLarge(this, !Options.ringLarge(this))
                placeRing()
            }
        }
        show()
    }

    /** The ring's size: large fills the width (half the screen across); small leaves the list more room. */
    private fun placeRing() {
        val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
        val large = Options.ringLarge(this)
        ring.layoutParams = if (landscape) {
            LinearLayout.LayoutParams(0, -1, if (large) 1f else 0.6f)
        } else {
            val side = if (large) -2 else (resources.displayMetrics.widthPixels * 0.55f).toInt()
            LinearLayout.LayoutParams(-1, side).apply { gravity = Gravity.CENTER_HORIZONTAL }
        }
    }

    /** The settings that change less often than Start and Mine, each labeled with its current value. */
    private fun settings() {
        val items = listOf<Pair<String, () -> Unit>>(
            (if (Options.classic(this)) "Classic: on" else "Classic: off") to ::toggleClassic,
            (if (Options.link(this)) "Laptop: on" else "Laptop: off") to ::toggleLaptop,
            "Keep ${Recordings.retentionDays(this)} d" to ::cycleRetention,
            "Delete all" to {
                if (Live.running) status.text = "Stop the session before deleting saved feeds." else confirmDeleteAll()
            },
            "About" to { About.show(this) },
        )
        AlertDialog.Builder(this)
            .setTitle("Settings")
            .setItems(items.map { it.first }.toTypedArray()) { _, which -> items[which].second() }
            .setNegativeButton("Close", null)
            .show()
    }

    private fun line(e: Emitter): String {
        val dbm = e.latestDbm?.let { "%4.0f".format(it) } ?: "   ?"
        val what = listOfNotNull(
            e.tracker?.let { Labels.TRACKERS[it] ?: it },
            e.category,
            e.ssid?.let { "\"$it\"" },
            e.vendor,
            e.security,
            if (e.address == "randomized") "randomized" else null,
        ).joinToString(", ")
        val head = "%-6s %s %-4s %s".format(e.band, dbm, Labels.KINDS[e.kind] ?: e.kind, e.device.substringAfter(':'))
        return if (what.isEmpty()) head else "$head\n       $what"
    }

    private fun start() {
        if (!Recordings.coreBinary(this).exists()) {
            status.text = "The core isn't in this build: run android/build-core.sh, then build the app again."
            return
        }
        val missing = PERMISSIONS.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isNotEmpty()) {
            requestPermissions(missing.toTypedArray(), 1)
            return
        }
        startForegroundService(Intent(this, SessionService::class.java))
    }

    private fun stop() {
        stopService(Intent(this, SessionService::class.java))
    }

    /**
     * Bluetooth Classic discovery, off by default. Turning it on asks for
     * BLUETOOTH_CONNECT, which telling a Classic device from a BLE one needs.
     */
    private fun toggleClassic() {
        if (Options.classic(this)) {
            Options.setClassic(this, false)
        } else if (checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED) {
            Options.setClassic(this, true)
        } else {
            requestPermissions(arrayOf(Manifest.permission.BLUETOOTH_CONNECT), CLASSIC_REQUEST)
            return
        }
        show()
    }

    /**
     * The laptop link, off by default; a running session picks the change up
     * within seconds. Turning it off can also forget the paired laptop.
     */
    private fun toggleLaptop() {
        if (!Options.link(this)) {
            Options.setLink(this, true)
            show()
            return
        }
        val paired = Pairing.get(this)?.first
        val choices = listOfNotNull("Turn off", paired?.let { "Turn off and forget $it" })
        AlertDialog.Builder(this)
            .setTitle("Laptop link")
            .setItems(choices.toTypedArray()) { _, which ->
                Options.setLink(this, false)
                if (which == 1) Pairing.forget(this)
                show()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        if (requestCode == CLASSIC_REQUEST) {
            val granted = checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED
            Options.setClassic(this, granted)
            if (!granted) status.text = "Bluetooth Classic discovery needs the nearby devices permission to connect."
            show()
            return
        }
        val needed = PERMISSIONS - Manifest.permission.POST_NOTIFICATIONS
        val refused = needed.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (refused.isEmpty()) {
            startForegroundService(Intent(this, SessionService::class.java))
        } else {
            status.text = "A session needs location, nearby devices and Wi-Fi scanning: " +
                refused.joinToString { it.substringAfterLast('.') }
        }
    }

    /** The user picks their own emitters; the loudest are offered first. */
    private fun chooseMine() {
        val enrolled = Enrolled.devices(this)
        val offer = Enrolled.offer(Live.picture, enrolled)
        if (offer.isEmpty()) {
            status.text = "Start a session first: the emitters it hears are offered here, loudest first."
            return
        }
        val labels = offer.map { (e, device) ->
            when {
                e == null -> "${device.substringAfter(':')} (not heard in the last minute)"
                e.tracker != null -> "TRACKER " + oneLine(line(e))
                else -> oneLine(line(e))
            }
        }.toTypedArray()
        val checked = offer.map { (e, device) -> device in enrolled || Enrolled.suggested(e, enrolled) }.toBooleanArray()
        AlertDialog.Builder(this)
            .setTitle("Which are yours?")
            .setMultiChoiceItems(labels, checked) { _, which, isChecked -> checked[which] = isChecked }
            .setPositiveButton("Save") { _, _ ->
                Enrolled.set(this, offer.indices.filter { checked[it] }.map { offer[it].second }.toSet())
                show()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun oneLine(s: String) = s.replace(Regex("\\s+"), " ")

    private fun cycleRetention() {
        val choices = Recordings.RETENTION_CHOICES
        val next = choices[(choices.indexOf(Recordings.retentionDays(this)) + 1) % choices.size]
        Recordings.setRetentionDays(this, next)
        show()
    }

    private fun confirmDeleteAll() {
        val count = Recordings.files(this).size
        AlertDialog.Builder(this)
            .setTitle("Delete all saved feeds?")
            .setMessage("$count saved session(s) on this phone. This can't be undone.")
            .setPositiveButton("Delete") { _, _ ->
                val deleted = Recordings.deleteAll(this)
                status.text = "Deleted $deleted saved session(s)."
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    companion object {
        const val LIST_LENGTH = 40
        private const val CLASSIC_REQUEST = 2

        val PERMISSIONS = listOf(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.BLUETOOTH_SCAN,
            Manifest.permission.NEARBY_WIFI_DEVICES,
            Manifest.permission.POST_NOTIFICATIONS,
        )
    }
}

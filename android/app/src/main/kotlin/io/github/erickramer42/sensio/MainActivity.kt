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
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * The screen: the ring, the loudest emitters as a list, the session's
 * coverage, and the controls. It stays on while a session runs (roadmap item
 * 69): the driver keeps the full picture, and BLE keeps its match-everything
 * scan.
 */
class MainActivity : Activity() {
    private lateinit var ring: RingView
    private lateinit var status: TextView
    private lateinit var list: TextView
    private lateinit var startStop: Button
    private lateinit var retention: Button
    private lateinit var deleteAll: Button
    private val refresh: () -> Unit = { show() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
        val pad = (12 * resources.displayMetrics.density).toInt()

        ring = RingView(this)
        status = text(13f)
        list = text(12f).apply { typeface = Typeface.MONOSPACE }
        startStop = button("Start") { if (Live.running) stop() else start() }
        retention = button("") { cycleRetention() }
        deleteAll = button("Delete all") { confirmDeleteAll() }

        val buttons = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            listOf(startStop, retention, deleteAll).forEach { addView(it, LinearLayout.LayoutParams(0, -2, 1f)) }
        }
        val side = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(buttons)
            addView(status)
            addView(ScrollView(this@MainActivity).apply { addView(list) }, LinearLayout.LayoutParams(-1, 0, 1f))
        }
        val root = LinearLayout(this).apply {
            orientation = if (landscape) LinearLayout.HORIZONTAL else LinearLayout.VERTICAL
            setBackgroundColor(Color.rgb(18, 20, 24))
            setPadding(pad, pad, pad, pad)
            if (landscape) {
                addView(ring, LinearLayout.LayoutParams(0, -1, 1f))
                addView(side, LinearLayout.LayoutParams(0, -1, 1f))
            } else {
                addView(ring, LinearLayout.LayoutParams(-1, -2))
                addView(side, LinearLayout.LayoutParams(-1, 0, 1f))
            }
        }
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
        setOnClickListener { action() }
    }

    private fun show() {
        val running = Live.running
        if (running) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        startStop.text = if (running) "Stop" else "Start"
        retention.text = "Keep ${Recordings.retentionDays(this)} d"
        deleteAll.isEnabled = !running

        val picture = Live.picture
        ring.picture = picture
        val coverage = Live.coverage
        status.text = buildString {
            append(Live.status)
            if (picture != null) {
                append("\n${picture.emitters.size} emitters in the last ${picture.recentS} s, ${picture.older} older; ${picture.movement}")
            }
            if (coverage != null) {
                append("\n${coverage.lines} lines, ${coverage.events} events, ${coverage.rejected} rejected")
                if (coverage.withheld + coverage.dropped > 0) append(", ${coverage.dropped} not passed to the core")
                if (coverage.sleptS > 0) append(", asleep ${coverage.sleptS} s")
            }
            append("\nRings: signal strength, not distance. Angle: kind, not direction.")
        }
        list.text = picture?.emitters?.take(LIST_LENGTH)?.joinToString("\n") { line(it) } ?: ""
    }

    private fun line(e: Emitter): String {
        val dbm = e.latestDbm?.let { "%4.0f".format(it) } ?: "   ?"
        val what = listOfNotNull(
            e.tracker?.let { TRACKERS[it] ?: it },
            e.category,
            e.ssid?.let { "\"$it\"" },
            e.vendor,
            e.security,
            if (e.address == "randomized") "randomized" else null,
        ).joinToString(", ")
        val head = "%-6s %s %-4s %s".format(e.band, dbm, KINDS[e.kind] ?: e.kind, e.device.substringAfter(':'))
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

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        val needed = PERMISSIONS - Manifest.permission.POST_NOTIFICATIONS
        val refused = needed.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (refused.isEmpty()) {
            startForegroundService(Intent(this, SessionService::class.java))
        } else {
            status.text = "A session needs location, nearby devices and Wi-Fi scanning: " +
                refused.joinToString { it.substringAfterLast('.') }
        }
    }

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

        val PERMISSIONS = listOf(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.BLUETOOTH_SCAN,
            Manifest.permission.NEARBY_WIFI_DEVICES,
            Manifest.permission.POST_NOTIFICATIONS,
        )

        private val KINDS = mapOf("ble" to "BLE", "access_point" to "AP", "station" to "STA", "tpms" to "TPMS")

        private val TRACKERS = mapOf(
            "apple_find_my" to "Find My",
            "google_find_hub" to "Find Hub",
            "dult" to "DULT tracker",
        )
    }
}

package io.github.erickramer42.sensio

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.bluetooth.BluetoothClass
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.net.wifi.WifiManager
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.ParcelUuid
import android.os.PowerManager
import android.os.SystemClock
import java.io.BufferedWriter
import java.io.File

/**
 * A session (ADR-0004 section 7): a foreground service of type `location`
 * with a partial wake lock, reading Android's Wi-Fi scan results, BLE scan
 * results and GNSS fixes, writing each as a feed line to the session's file
 * and to the core, and asking the core for the picture every few seconds.
 */
@SuppressLint("MissingPermission")
class SessionService : Service() {
    private lateinit var worker: HandlerThread
    private lateinit var handler: Handler
    private var wakeLock: PowerManager.WakeLock? = null
    private var core: Core? = null
    private var link: Link? = null
    private var file: BufferedWriter? = null
    private lateinit var feed: File
    private var started = false
    private var restarts = 0
    private var coreStartedMs = 0L

    private var seq = 0L
    /** Lines not passed to the core, by source and receiver, until a `dropped` line says so. */
    private val pendingDrops = LinkedHashMap<Pair<String, String>, Long>()
    private var sleptMs = 0L
    private var gnssClockAtMs: Long? = null
    private var bleFiltered = false
    private val wifiSeen = HashMap<String, Long>()

    private val bleCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            data(Feed.bleRaw(result.device.address, result.rssi, result.scanRecord?.bytes, result.timestampNanos))
        }

        override fun onBatchScanResults(results: MutableList<ScanResult>) = results.forEach { onScanResult(0, it) }

        override fun onScanFailed(errorCode: Int) = Live.status("BLE scan failed (code $errorCode)")
    }

    private val wifiReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = wifiResults()
    }

    private val screenReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = startBle(filtered = intent.action == Intent.ACTION_SCREEN_OFF)
    }

    private val locationListener = LocationListener { location -> fix(location) }

    private val classicReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            // An error here must never take the session down with it.
            runCatching { found(intent) }.onFailure { Live.status("Bluetooth Classic discovery failed: ${it.javaClass.simpleName}") }
        }

        private fun found(intent: Intent) {
            if (intent.action != BluetoothDevice.ACTION_FOUND) return
            val device = intent.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE, BluetoothDevice::class.java) ?: return
            // A BLE-only device is the BLE scan's; discovery reports it too.
            // Reading the type needs BLUETOOTH_CONNECT.
            if (device.type == BluetoothDevice.DEVICE_TYPE_LE) return
            val rssi = intent.getShortExtra(BluetoothDevice.EXTRA_RSSI, Short.MIN_VALUE).toInt().takeIf { it != Short.MIN_VALUE.toInt() }
            val deviceClass = intent.getParcelableExtra(BluetoothDevice.EXTRA_CLASS, BluetoothClass::class.java)?.deviceClass
            data(Feed.btRaw(device.address, rssi, deviceClass))
        }
    }

    /** Bluetooth Classic discovery, when the user has turned it on; each round takes about 12 s. */
    private val classicScan = object : Runnable {
        override fun run() {
            val adapter = getSystemService(BluetoothManager::class.java).adapter
            val allowed = checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED
            if (Options.classic(this@SessionService) && allowed && adapter != null && !adapter.isDiscovering) {
                runCatching { adapter.startDiscovery() }
            }
            handler.postDelayed(this, CLASSIC_EVERY_MS)
        }
    }

    private val tick = object : Runnable {
        override fun run() {
            val slept = SystemClock.elapsedRealtime() - SystemClock.uptimeMillis()
            val boot = SystemClock.elapsedRealtimeNanos()
            if (slept - sleptMs >= 1_000) control(Feed.slept(boot, (slept - sleptMs) * 1_000_000))
            sleptMs = slept
            control(Feed.judge(boot))
            synchronized(this@SessionService) { file?.flush() }
            handler.postDelayed(this, JUDGE_EVERY_MS)
        }
    }

    private val wifiScan = object : Runnable {
        override fun run() {
            @Suppress("DEPRECATION")
            getSystemService(WifiManager::class.java).startScan()
            handler.postDelayed(this, WIFI_EVERY_MS)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (started) return START_NOT_STICKY
        started = true
        startForeground(1, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION)
        wakeLock = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "peil:session").apply { acquire() }
        worker = HandlerThread("session").apply { start() }
        handler = Handler(worker.looper)
        Recordings.prune(this)
        Live.clear()

        startCore(restart = false)
        feed = Recordings.newFeed(this)
        file = feed.bufferedWriter()
        sleptMs = SystemClock.elapsedRealtime() - SystemClock.uptimeMillis()
        val boot = SystemClock.elapsedRealtimeNanos()
        control(Feed.start("peil-android ${BuildInfo.version(this)}", boot))
        control(Feed.clock(boot, System.currentTimeMillis() * 1_000_000, gnss = false))

        val wifi = getSystemService(WifiManager::class.java)
        registerReceiver(wifiReceiver, IntentFilter(WifiManager.SCAN_RESULTS_AVAILABLE_ACTION), null, handler)
        registerReceiver(
            screenReceiver,
            IntentFilter().apply {
                addAction(Intent.ACTION_SCREEN_ON)
                addAction(Intent.ACTION_SCREEN_OFF)
            },
            null,
            handler,
        )
        startBle(filtered = !getSystemService(PowerManager::class.java).isInteractive)
        registerReceiver(classicReceiver, IntentFilter(BluetoothDevice.ACTION_FOUND), null, handler)
        handler.post(classicScan)
        handler.post(wifiScan)
        getSystemService(LocationManager::class.java)
            .requestLocationUpdates(LocationManager.GPS_PROVIDER, 1_000L, 0f, locationListener, worker.looper)
        handler.postDelayed(tick, JUDGE_EVERY_MS)
        link = Link(
            this,
            line = { source, receiver, remoteNs, raw -> data(raw, source, receiver, remoteNs) },
            offset = { receiver, offsetNs, uncertaintyNs -> control(Feed.offset(receiver, offsetNs, uncertaintyNs)) },
        )
        val throttle = if (wifi.isScanThrottleEnabled) "on (4 scans per 2 minutes)" else "off"
        if (core != null) Live.status("Session running; Wi-Fi scan throttling $throttle", running = true)
        return START_NOT_STICKY
    }

    /**
     * A match-everything BLE filter while the screen is on; specific ones while
     * it's off, since Android pauses a match-everything scan then (ADR-0004,
     * amended 2026-11-06).
     */
    private fun startBle(filtered: Boolean) {
        val scanner = getSystemService(BluetoothManager::class.java).adapter?.bluetoothLeScanner
        if (scanner == null) {
            Live.status("No BLE scanner (is Bluetooth off?)")
            return
        }
        if (bleRunning && bleFiltered == filtered) return
        if (bleRunning) runCatching { scanner.stopScan(bleCallback) }
        val filters = if (!filtered) {
            listOf(ScanFilter.Builder().build())
        } else {
            SCREEN_OFF_COMPANIES.map { ScanFilter.Builder().setManufacturerData(it, ByteArray(0)).build() } +
                SCREEN_OFF_SERVICES.map {
                    ScanFilter.Builder().setServiceUuid(ParcelUuid.fromString("%08x-0000-1000-8000-00805f9b34fb".format(it))).build()
                }
        }
        val settings = ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build()
        scanner.startScan(filters, settings, bleCallback)
        bleRunning = true
        bleFiltered = filtered
        if (filtered) Live.status("Screen off: BLE sees only the listed manufacturers and tracker services")
    }

    private var bleRunning = false

    /** The scan's new results: Android repeats a result until it's seen again. */
    private fun wifiResults() {
        val results = getSystemService(WifiManager::class.java).scanResults
        val fresh = results.filter { wifiSeen.put(it.BSSID, it.timestamp) != it.timestamp }.map {
            @Suppress("DEPRECATION")
            Feed.WifiResult(it.BSSID, it.SSID, it.frequency, it.level, it.capabilities, it.timestamp)
        }
        if (fresh.isNotEmpty()) data(Feed.wifiRaw(fresh))
    }

    private fun fix(location: Location) {
        val boot = SystemClock.elapsedRealtimeNanos()
        val now = SystemClock.elapsedRealtime()
        if (location.provider == LocationManager.GPS_PROVIDER && gnssClockAtMs.let { it == null || now - it >= CLOCK_EVERY_MS }) {
            // GNSS time at the fix, carried forward to now on the boot clock.
            val utc = location.time * 1_000_000 + (boot - location.elapsedRealtimeNanos)
            control(Feed.clock(boot, utc, gnss = true))
            gnssClockAtMs = now
        }
        data(
            Feed.locRaw(
                location.latitude,
                location.longitude,
                location.provider,
                location.accuracy.takeIf { location.hasAccuracy() },
                location.speed.takeIf { location.hasSpeed() },
                location.bearing.takeIf { location.hasBearing() },
            ),
        )
    }

    /**
     * A feed line other than a source's: to the core and the file both, or,
     * if even the reserve is full, to neither, so the file stays what the
     * core saw.
     */
    @Synchronized
    private fun control(line: String) {
        val c = core
        if (c == null || c.offer(line)) write(line)
    }

    /**
     * A source's raw line. When the core's buffer is full, it goes to the file
     * only, withheld, and a `dropped` line with the count follows once there's
     * room (ADR-0005 section 5).
     */
    @Synchronized
    private fun data(raw: String, source: String = Feed.SOURCE, receiver: String = Feed.RECEIVER, remoteNs: Long? = null) {
        if (file == null) return
        val boot = SystemClock.elapsedRealtimeNanos()
        seq += 1
        val c = core
        val pending = pendingDrops.iterator()
        while (c != null && pending.hasNext() && c.hasRoom(RESERVE + 2)) {
            val (key, lines) = pending.next()
            val dropped = Feed.dropped(lines, key.first, key.second)
            if (!c.offer(dropped)) break
            write(dropped)
            pending.remove()
        }
        val line = Feed.line(seq, boot, raw, source = source, receiver = receiver, remoteNs = remoteNs)
        if (c != null && pendingDrops.isEmpty() && c.hasRoom(RESERVE + 1) && c.offer(line)) {
            write(line)
        } else {
            write(Feed.line(seq, boot, raw, withheld = true, source = source, receiver = receiver, remoteNs = remoteNs))
            pendingDrops.merge(Pair(source, receiver), 1L, Long::plus)
        }
    }

    private fun write(line: String) {
        file?.apply {
            write(line)
            write('\n'.code)
        }
    }

    private fun fromCore(line: String) {
        when (val parsed = VerdictStream.parse(line)) {
            is Picture -> Live.update(picture = parsed)
            is Coverage -> Live.update(coverage = parsed)
        }
    }

    /**
     * Starts the core. A restarted one first reads the saved feed so far, so
     * its picture goes on from the stopped one's; the lines that arrived in
     * between were written withheld, and the `dropped` count that follows
     * shows the gap as lost coverage (ADR-0004).
     */
    @Synchronized
    private fun startCore(restart: Boolean) {
        if (!started || core != null) return
        val prelude = if (restart) {
            file?.flush()
            Core.Prelude(feed, feed.length())
        } else {
            null
        }
        core = runCatching { Core(Recordings.coreBinary(this), ::fromCore, ::coreExited, prelude) }
            .onFailure { Live.status("The core didn't start: ${it.message}", running = true) }
            .getOrNull()
        coreStartedMs = SystemClock.elapsedRealtime()
        if (restart && core != null) Live.status("The core restarted; the lines it missed are counted as not passed to it")
    }

    /** A core that stops during a session is restarted, after 1 s, then up to a minute if it keeps stopping. */
    private fun coreExited(stopped: Core, code: Int, err: String) {
        synchronized(this) {
            if (core !== stopped) return
            core = null
            if (!started) return
            if (SystemClock.elapsedRealtime() - coreStartedMs > RESTART_RESET_MS) restarts = 0
            val delay = minOf(RESTART_MAX_MS, 1_000L shl minOf(restarts, 6))
            restarts += 1
            Live.status("The core stopped (exit $code)${if (err.isEmpty()) "" else ": $err"}; restarting in ${delay / 1000} s")
            handler.postDelayed({ startCore(restart = true) }, delay)
        }
    }

    private fun notification(): Notification {
        val channel = NotificationChannel("session", "Session", NotificationManager.IMPORTANCE_LOW)
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, "session")
            .setContentTitle("Peil is listening")
            .setSmallIcon(android.R.drawable.ic_menu_mylocation)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
    }

    override fun onDestroy() {
        if (started) {
            started = false
            handler.removeCallbacksAndMessages(null)
            link?.close()
            link = null
            runCatching { getSystemService(BluetoothManager::class.java).adapter?.bluetoothLeScanner?.stopScan(bleCallback) }
            runCatching { unregisterReceiver(wifiReceiver) }
            runCatching { unregisterReceiver(screenReceiver) }
            runCatching { unregisterReceiver(classicReceiver) }
            runCatching { getSystemService(BluetoothManager::class.java).adapter?.cancelDiscovery() }
            runCatching { getSystemService(LocationManager::class.java).removeUpdates(locationListener) }
            synchronized(this) {
                core?.close()
                runCatching { file?.close() }
                file = null
            }
            worker.quitSafely()
            wakeLock?.takeIf { it.isHeld }?.release()
            Live.status("Session stopped", running = false)
        }
        super.onDestroy()
    }

    companion object {
        const val JUDGE_EVERY_MS = 3_000L
        const val WIFI_EVERY_MS = 10_000L
        const val CLASSIC_EVERY_MS = 30_000L
        const val CLOCK_EVERY_MS = 60_000L
        const val RESTART_MAX_MS = 60_000L

        /** A core that ran this long before stopping starts the backoff over. */
        const val RESTART_RESET_MS = 300_000L

        /** Buffer room kept for the session's own lines (judge, clock, slept). */
        const val RESERVE = 64

        /**
         * Screen-off BLE filters, from the spike on the test phone: Apple,
         * Samsung, Google and Microsoft's company IDs, and the tracker and
         * accessory services (Samsung's FD5A, Tile's FEED and FEEC, DULT's
         * FCB2, Google's FEAA and FE2C).
         */
        val SCREEN_OFF_COMPANIES = listOf(0x004C, 0x0075, 0x00E0, 0x0006)
        val SCREEN_OFF_SERVICES = listOf(0xFD5A, 0xFEED, 0xFEEC, 0xFCB2, 0xFEAA, 0xFE2C)
    }
}

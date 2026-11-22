package io.github.erickramer42.sensio

import android.os.Handler
import android.os.Looper
import org.json.JSONObject

/** One emitter from the core's `emitters` line (ADR-0006): what it is, by lookup and measurement. */
class Emitter(
    val device: String,
    val kind: String,
    val band: String,
    val latestDbm: Double?,
    val strongestDbm: Double?,
    val sightings: Long,
    val address: String,
    val ssid: String?,
    val vendor: String?,
    val tracker: String?,
    val security: String?,
    val category: String?,
    val firstSeenNs: Long = 0,
    /** The table entry behind [category], which names an advert's device where its format does. */
    val categoryEntry: String? = null,
    /** The receiver that gave this address as its own; a label only, since another device can copy an address. */
    val own: String? = null,
    /** What an aircraft last announced and where it is from the vehicle; null for any other kind. */
    val aircraft: Aircraft? = null,
    /** What a drone's Remote ID adverts said; null for an emitter that sent none. */
    val drone: Drone? = null,
)

/**
 * An aircraft's latest announcements (ADR-0007), in the units the core gives:
 * feet and knots, as aviation states them, and meters from the vehicle.
 * Bearings are degrees clockwise from true north.
 */
class Aircraft(
    val callsign: String? = null,
    val altBaroFt: Long? = null,
    val onGround: Boolean = false,
    val groundSpeedKt: Double? = null,
    val trackDeg: Double? = null,
    val distanceM: Double? = null,
    val bearingDeg: Double? = null,
    val squawk: String? = null,
    val emergency: String? = null,
    val signalDbfs: Double? = null,
    /** The FAA snapshot's answer: null outside the US block, else found or not. */
    val registry: Registration? = null,
)

/** The FAA snapshot's entry for an aircraft; [found] false when its address is in the US block but not listed. */
class Registration(
    val found: Boolean,
    val nNumber: String? = null,
    val make: String? = null,
    val model: String? = null,
    val year: Long? = null,
    /** The registrant's name, which the snapshot keeps only for an organization. */
    val owner: String? = null,
)

/** A drone's latest Remote ID messages (ADR-0007 section 6), meters and m/s, bearings from true north. */
class Drone(
    val uasId: String? = null,
    /** `ODID_status_t`: 1 ground, 2 airborne, 3 emergency, 4 Remote ID failure. */
    val status: Long? = null,
    val heightM: Double? = null,
    val speedMps: Double? = null,
    val distanceM: Double? = null,
    val bearingDeg: Double? = null,
    val operatorDistanceM: Double? = null,
    val operatorBearingDeg: Double? = null,
    val operatorId: String? = null,
)

/** The core's ambient texture: emitters in the recent window by kind, those first seen in it, and the newest. */
class Texture(
    val counts: Map<String, Long>,
    val new: Map<String, Long>,
    val randomizedWifi: Long,
    val newest: List<String>,
)

/** The emitter picture at one moment, loud first, as the core described it. */
class Picture(
    val movement: String,
    val emitters: List<Emitter>,
    val older: Long,
    val recentS: Long,
    val atNs: Long = 0,
    val recentNs: Long = recentS * 1_000_000_000L,
    val texture: Texture? = null,
)

/** Short names for the core's emitter kinds and tracker networks. */
object Labels {
    val KINDS = mapOf("ble" to "BLE", "bt_classic" to "BT", "access_point" to "AP", "station" to "STA", "aircraft" to "AIR")

    val TRACKERS = mapOf(
        "apple_find_my" to "Find My",
        "google_find_hub" to "Find Hub",
        "dult" to "DULT tracker",
        "tile" to "Tile",
        "smartthings_find" to "SmartThings",
    )

    /** The devices a tracker format's category entry names. */
    val DEVICES = mapOf(
        "apple-find-my-airtag" to "AirTag",
        "apple-find-my-accessory" to "accessory",
        "apple-find-my-airpods" to "AirPods",
        "apple-find-my-apple-device" to "Apple device",
        "samsung-smarttag" to "SmartTag",
    )

    /** An emitter's tracker network, with the device its advert names: "Find My AirTag". */
    fun tracker(e: Emitter): String? = e.tracker?.let { t ->
        listOfNotNull(TRACKERS[t] ?: t, DEVICES[e.categoryEntry]).joinToString(" ")
    }
}

/** The verdict stream's lines the display uses. Other types are skipped, as ADR-0005 says readers do. */
object VerdictStream {
    fun parse(line: String): Any? {
        val o = runCatching { JSONObject(line) }.getOrNull() ?: return null
        return when (o.optString("type")) {
            "emitters" -> picture(o)
            "coverage" -> coverage(o)
            "find" -> find(o)
            else -> null
        }
    }

    private fun JSONObject.text(key: String): String? = if (has(key) && !isNull(key)) optString(key) else null

    private fun JSONObject.number(key: String): Double? = if (has(key) && !isNull(key)) optDouble(key) else null

    private fun JSONObject.whole(key: String): Long? = if (has(key) && !isNull(key)) optLong(key) else null

    private fun aircraft(a: JSONObject) = Aircraft(
        callsign = a.text("callsign"),
        altBaroFt = a.whole("alt_baro_ft"),
        onGround = a.optBoolean("on_ground"),
        groundSpeedKt = a.number("ground_speed_kt"),
        trackDeg = a.number("track_deg"),
        distanceM = a.number("distance_m"),
        bearingDeg = a.number("bearing_deg"),
        squawk = a.text("squawk"),
        emergency = a.text("emergency"),
        signalDbfs = a.number("signal_dbfs"),
        registry = a.optJSONObject("registry")?.let { r ->
            Registration(
                found = r.optString("lookup") == "found",
                nNumber = r.text("n_number"),
                make = r.text("make"),
                model = r.text("model"),
                year = r.whole("year"),
                owner = r.text("owner"),
            )
        },
    )

    private fun drone(d: JSONObject) = Drone(
        uasId = d.text("uas_id"),
        status = d.whole("status"),
        heightM = d.number("height_m"),
        speedMps = d.number("speed_mps"),
        distanceM = d.number("distance_m"),
        bearingDeg = d.number("bearing_deg"),
        operatorDistanceM = d.number("operator_distance_m"),
        operatorBearingDeg = d.number("operator_bearing_deg"),
        operatorId = d.text("operator_id"),
    )

    private fun find(o: JSONObject): FindAnswer {
        val sectors = o.optJSONArray("sectors")
        return FindAnswer(
            device = o.optString("device"),
            status = o.optString("status"),
            bearingDeg = o.number("bearing_deg"),
            contrastDb = o.number("contrast_db"),
            paired = o.optLong("paired"),
            unpaired = o.optLong("unpaired"),
            latestDbm = o.number("latest_dbm"),
            covered = (0 until (sectors?.length() ?: 0)).map { sectors!!.getJSONObject(it).optLong("readings") > 0 },
        )
    }

    private fun picture(o: JSONObject): Picture {
        val list = o.optJSONArray("emitters")
        val emitters = (0 until (list?.length() ?: 0)).map { i ->
            val e = list!!.getJSONObject(i)
            val security = e.optJSONObject("security")?.let {
                it.optString("protocol") + if (it.optBoolean("enterprise")) " enterprise" else ""
            }
            Emitter(
                device = e.optString("device"),
                kind = e.optString("kind"),
                band = e.optString("band", "unknown"),
                latestDbm = e.number("latest_dbm"),
                strongestDbm = e.number("strongest_dbm"),
                sightings = e.optLong("sightings"),
                address = e.optString("address"),
                ssid = e.text("ssid"),
                vendor = e.text("vendor"),
                tracker = e.text("tracker"),
                security = security,
                category = e.optJSONObject("category")?.optString("category"),
                firstSeenNs = e.optLong("first_seen_ns"),
                categoryEntry = e.optJSONObject("category")?.optString("entry")?.ifEmpty { null },
                own = e.text("own"),
                aircraft = e.optJSONObject("aircraft")?.let(::aircraft),
                drone = e.optJSONObject("drone")?.let(::drone),
            )
        }
        val recentNs = o.optLong("recent_ns")
        return Picture(
            o.optString("movement", "unknown"), emitters, o.optLong("older"), recentNs / 1_000_000_000L,
            atNs = o.optLong("at_ns"),
            recentNs = recentNs,
            texture = o.optJSONObject("texture")?.let(::texture),
        )
    }

    private fun JSONObject?.counts(): Map<String, Long> =
        this?.keys()?.asSequence()?.associateWith { optLong(it) } ?: emptyMap()

    private fun texture(t: JSONObject): Texture {
        val newest = t.optJSONArray("newest")
        return Texture(
            counts = t.optJSONObject("counts").counts(),
            new = t.optJSONObject("new").counts(),
            randomizedWifi = t.optLong("randomized_wifi"),
            newest = (0 until (newest?.length() ?: 0)).map { newest!!.optString(it) },
        )
    }

    private fun coverage(o: JSONObject): Coverage {
        val sources = o.optJSONArray("sources")
        var lines = 0L
        var events = 0L
        var rejected = 0L
        var withheld = 0L
        var dropped = 0L
        var late = 0L
        val lastArrival = HashMap<String, HashMap<String, Long>>()
        for (i in 0 until (sources?.length() ?: 0)) {
            val s = sources!!.getJSONObject(i)
            lines += s.optLong("lines")
            events += s.optLong("events")
            rejected += s.optLong("rejected")
            withheld += s.optLong("withheld")
            dropped += s.optLong("dropped")
            late += s.optLong("late")
            val receiver = s.text("receiver") ?: s.optString("source")
            val kinds = s.optJSONObject("kinds") ?: continue
            val byKind = lastArrival.getOrPut(receiver) { HashMap() }
            for (kind in kinds.keys()) {
                val last = kinds.optJSONObject(kind)?.optLong("last_boot_ns") ?: continue
                byKind.merge(kind, last, ::maxOf)
            }
        }
        val list = o.optJSONArray("radios")
        val radios = (0 until (list?.length() ?: 0)).map { i ->
            val r = list!!.getJSONObject(i)
            RadioCoverage(
                radio = r.optString("radio"),
                receiver = r.text("receiver") ?: "",
                state = r.optString("state"),
                filteredS = r.optLong("filtered_ns") / 1_000_000_000L,
                lostS = r.optLong("lost_ns") / 1_000_000_000L,
                lowRate = r.optBoolean("low_rate"),
                lowS = r.optLong("low_ns") / 1_000_000_000L,
                lastMinute = r.optJSONObject("last_minute")?.optLong("results"),
            )
        }
        return Coverage(
            lines, events, rejected, withheld, dropped, o.optLong("slept_ns") / 1_000_000_000L,
            bootNs = if (o.has("boot_ns")) o.optLong("boot_ns") else null,
            lastArrival = lastArrival,
            radios = radios,
            late = late,
        )
    }
}

/** What the running session shows, handed from the service to the screen on the main thread. */
object Live {
    @Volatile var picture: Picture? = null
        private set
    @Volatile var coverage: Coverage? = null
        private set
    @Volatile var status: String = "No session"
        private set
    @Volatile var running: Boolean = false
        private set
    /** The laptop link's state, or null when it's off. */
    @Volatile var link: String? = null
        private set
    /** Each device a linked receiver gave as its own, with that receiver: offered in the Mine list, unchecked. */
    @Volatile var own: Map<String, String> = emptyMap()
        private set
    /** The device find mode looks for, or null when it's off. */
    @Volatile var findTarget: String? = null
        private set
    /** When its sweep started, on the boot clock. */
    @Volatile var findFromBootNs: Long = 0
        private set
    /** The core's latest answer for [findTarget]. */
    @Volatile var findAnswer: FindAnswer? = null
        private set
    /** The phone's latest azimuth, degrees from magnetic north, while the screen is on. */
    @Volatile var headingDeg: Double? = null
        private set

    private val main = Handler(Looper.getMainLooper())
    private val listeners = mutableListOf<() -> Unit>()

    fun update(picture: Picture? = this.picture, coverage: Coverage? = this.coverage) {
        this.picture = picture
        this.coverage = coverage
        notifyListeners()
    }

    fun status(status: String, running: Boolean = this.running) {
        this.status = status
        this.running = running
        notifyListeners()
    }

    fun link(link: String?) {
        if (link == this.link) return
        this.link = link
        notifyListeners()
    }

    /** [receiver]'s own devices, replacing those it gave before. */
    fun own(receiver: String, devices: List<String>) {
        own = own.filterValues { it != receiver } + devices.associateWith { receiver }
        notifyListeners()
    }

    /** Starts finding [device], its sweep from [fromBootNs]. */
    fun startFind(device: String, fromBootNs: Long) {
        findTarget = device
        findFromBootNs = fromBootNs
        findAnswer = null
        notifyListeners()
    }

    fun stopFind() {
        findTarget = null
        findAnswer = null
        notifyListeners()
    }

    /** An answer from the core; one for a device no longer looked for is dropped. */
    fun find(answer: FindAnswer) {
        if (answer.device != findTarget) return
        findAnswer = answer
        notifyListeners()
    }

    /** The phone's heading; the screen hears of it only while finding, when the wedge turns with it. */
    fun heading(deg: Double) {
        headingDeg = deg
        if (findTarget != null) notifyListeners()
    }

    fun clear() {
        picture = null
        coverage = null
        own = emptyMap()
        findTarget = null
        findAnswer = null
    }

    private fun notifyListeners() {
        main.post { synchronized(listeners) { listeners.toList() }.forEach { it() } }
    }

    fun listen(listener: () -> Unit) = synchronized(listeners) { listeners += listener }

    fun unlisten(listener: () -> Unit) = synchronized(listeners) { listeners -= listener }
}

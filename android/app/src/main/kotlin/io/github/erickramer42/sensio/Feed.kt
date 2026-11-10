package io.github.erickramer42.sensio

/**
 * The lines the app writes: the feed (`sensio.feed/1`, ADR-0005) to the core
 * and to the session's file, and inside each feed `line` the `android`
 * source's raw text (the core's `android.rs` reads it). The app writes what
 * Android reported and decides nothing; a change to a raw line's fields is
 * made here and in `android.rs` together.
 *
 * Plain Kotlin with no Android types, so the JVM unit tests cover it.
 */
object Feed {
    const val SCHEMA = "sensio.feed/1"
    const val SOURCE = "android"
    const val RECEIVER = "phone"

    fun start(app: String, bootNs: Long) =
        """{"type":"start","schema":"$SCHEMA","app":${quote(app)},"boot_ns":$bootNs}"""

    /** [gnss] when the UTC time came from a GNSS fix, otherwise the system clock. */
    fun clock(bootNs: Long, utcNs: Long, gnss: Boolean) =
        """{"type":"clock","boot_ns":$bootNs,"utc_ns":$utcNs,"basis":"${if (gnss) "gnss" else "system"}"}"""

    /** A receiver's clock offset: this phone's boot clock minus the receiver's own clock (ADR-0005). */
    fun offset(receiver: String, offsetNs: Long, uncertaintyNs: Long) =
        """{"type":"offset","receiver":${quote(receiver)},"offset_ns":$offsetNs,"uncertainty_ns":$uncertaintyNs}"""

    /** A source's line; [remoteNs] is a remote receiver's own stamp for it, on its clock. */
    fun line(
        seq: Long,
        bootNs: Long,
        raw: String,
        withheld: Boolean = false,
        source: String = SOURCE,
        receiver: String = RECEIVER,
        remoteNs: Long? = null,
    ) = """{"type":"line","seq":$seq,"boot_ns":$bootNs,"source":${quote(source)},"receiver":${quote(receiver)}""" +
        (if (remoteNs != null) ""","remote_ns":$remoteNs""" else "") +
        ""","raw":${quote(raw)}""" +
        (if (withheld) ""","withheld":true}""" else "}")

    fun judge(bootNs: Long) = """{"type":"judge","boot_ns":$bootNs}"""

    fun dropped(lines: Long, source: String = SOURCE, receiver: String = RECEIVER) =
        """{"type":"dropped","source":${quote(source)},"receiver":${quote(receiver)},"lines":$lines}"""

    fun slept(bootNs: Long, sleptNs: Long) = """{"type":"slept","boot_ns":$bootNs,"slept_ns":$sleptNs}"""

    /** The session was stopped: written last, so a feed without one was cut (ADR-0005). */
    fun end(bootNs: Long) = """{"type":"end","boot_ns":$bootNs}"""

    /** A receiver's own addresses, (radio, address) pairs as the link checked them (ADR-0004 section 6). */
    fun own(bootNs: Long, receiver: String, addresses: List<Pair<String, String>>) =
        """{"type":"own","boot_ns":$bootNs,"receiver":${quote(receiver)},"addresses":[""" +
            addresses.joinToString(",") { (radio, address) -> """{"radio":${quote(radio)},"address":${quote(address)}}""" } +
            "]}"

    /** A radio's states, as the core's `RadioState` names them. */
    enum class RadioState(val wire: String) { ON("on"), FILTERED("filtered"), OFF("off"), FAILED("failed"), UNUSED("unused") }

    /** A radio's state from [bootNs] on: `wifi`, `ble`, `gnss`, `bt_classic` or `link` (ADR-0005). */
    fun radio(bootNs: Long, radio: String, state: RadioState, receiver: String = RECEIVER) =
        """{"type":"radio","boot_ns":$bootNs,"radio":${quote(radio)},"receiver":${quote(receiver)},"state":"${state.wire}"}"""

    /** One Wi-Fi scan result, as `ScanResult` gave it. */
    class WifiResult(
        val bssid: String,
        val ssid: String?,
        val freqMhz: Int,
        val levelDbm: Int,
        val caps: String?,
        val timestampUs: Long,
    )

    /** One Wi-Fi scan's new results. */
    fun wifiRaw(results: List<WifiResult>): String = results.joinToString(",", """{"t":"wifi","results":[""", "]}") {
        buildString {
            append("""{"bssid":""").append(quote(it.bssid))
            it.ssid?.let { s -> append(""","ssid":""").append(quote(s)) }
            append(""","freq":""").append(it.freqMhz)
            append(""","level":""").append(it.levelDbm)
            it.caps?.let { c -> append(""","caps":""").append(quote(c)) }
            append(""","ts_us":""").append(it.timestampUs)
            append('}')
        }
    }

    /** One BLE scan result: the advert's bytes as `ScanRecord.getBytes` gave them. */
    fun bleRaw(addr: String, rssi: Int, record: ByteArray?, timestampNs: Long): String =
        """{"t":"ble","addr":${quote(addr)},"rssi":$rssi,"record":"${hex(record ?: ByteArray(0))}","ts_ns":$timestampNs}"""

    /** One Bluetooth Classic device found by discovery; [deviceClass] is `BluetoothClass.getDeviceClass`. */
    fun btRaw(addr: String, rssi: Int?, deviceClass: Int?): String = buildString {
        append("""{"t":"bt","addr":""").append(quote(addr))
        rssi?.let { append(""","rssi":""").append(it) }
        deviceClass?.let { append(""","class":""").append(it) }
        append('}')
    }

    /** Degrees to two decimals. */
    private fun degrees(v: Double): Double = Math.round(v * 100.0) / 100.0

    /** `getOrientation`'s azimuth (radians, -pi to pi) as degrees from 0 to under 360. */
    fun azimuth(radians: Float): Double {
        val d = degrees(Math.toDegrees(radians.toDouble()))
        val wrapped = ((d % 360.0) + 360.0) % 360.0
        return if (wrapped >= 360.0) 0.0 else degrees(wrapped).let { if (it >= 360.0) 0.0 else it }
    }

    /**
     * The phone's orientation from the rotation-vector sensor: azimuth from
     * magnetic north, pitch and roll in degrees, and the sensor's own heading
     * accuracy estimate when it gave one (the `android` source's `rot`).
     */
    fun rotRaw(azimuthDeg: Double, pitchDeg: Double, rollDeg: Double, accuracyDeg: Double?): String =
        buildString {
            append("""{"t":"rot","az":""").append(azimuthDeg)
            append(""","pitch":""").append(degrees(pitchDeg.coerceIn(-180.0, 180.0)))
            append(""","roll":""").append(degrees(rollDeg.coerceIn(-180.0, 180.0)))
            accuracyDeg?.let { append(""","acc":""").append(degrees(it.coerceIn(0.0, 180.0))) }
            append('}')
        }

    /** One location fix; the fields after `lon` stay in the raw line. */
    fun locRaw(lat: Double, lon: Double, provider: String?, accuracyM: Float?, speedMps: Float?, bearingDeg: Float?): String =
        buildString {
            append("""{"t":"loc","lat":""").append(lat).append(""","lon":""").append(lon)
            provider?.let { append(""","provider":""").append(quote(it)) }
            accuracyM?.let { append(""","acc":""").append(it) }
            speedMps?.let { append(""","speed":""").append(it) }
            bearingDeg?.let { append(""","bearing":""").append(it) }
            append('}')
        }

    fun hex(bytes: ByteArray): String {
        val digits = "0123456789abcdef"
        val out = StringBuilder(bytes.size * 2)
        for (b in bytes) {
            val v = b.toInt() and 0xff
            out.append(digits[v shr 4]).append(digits[v and 0xf])
        }
        return out.toString()
    }

    /** A JSON string literal. */
    fun quote(s: String): String {
        val out = StringBuilder(s.length + 2).append('"')
        for (c in s) {
            when {
                c == '"' -> out.append("\\\"")
                c == '\\' -> out.append("\\\\")
                c == '\n' -> out.append("\\n")
                c == '\r' -> out.append("\\r")
                c == '\t' -> out.append("\\t")
                c < ' ' -> out.append("\\u%04x".format(c.code))
                else -> out.append(c)
            }
        }
        return out.append('"').toString()
    }
}

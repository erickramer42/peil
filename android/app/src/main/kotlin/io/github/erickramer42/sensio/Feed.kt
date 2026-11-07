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

    fun line(seq: Long, bootNs: Long, raw: String, withheld: Boolean = false) =
        """{"type":"line","seq":$seq,"boot_ns":$bootNs,"source":"$SOURCE","receiver":"$RECEIVER","raw":${quote(raw)}""" +
            (if (withheld) ""","withheld":true}""" else "}")

    fun judge(bootNs: Long) = """{"type":"judge","boot_ns":$bootNs}"""

    fun dropped(lines: Long) = """{"type":"dropped","source":"$SOURCE","receiver":"$RECEIVER","lines":$lines}"""

    fun slept(bootNs: Long, sleptNs: Long) = """{"type":"slept","boot_ns":$bootNs,"slept_ns":$sleptNs}"""

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

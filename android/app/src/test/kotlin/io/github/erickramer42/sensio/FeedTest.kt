package io.github.erickramer42.sensio

import org.junit.Assert.assertEquals
import org.junit.Test

/** The feed lines and the `android` source's raw text, against what the core reads. */
class FeedTest {
    @Test
    fun aRawLineIsQuotedInsideItsFeedLine() {
        val raw = Feed.bleRaw("aa:aa:aa:00:00:01", -61, byteArrayOf(0x02, 0x01, 0x06), 5)
        assertEquals("""{"t":"ble","addr":"aa:aa:aa:00:00:01","rssi":-61,"record":"020106","ts_ns":5}""", raw)
        assertEquals(
            """{"type":"line","seq":3,"boot_ns":7,"source":"android","receiver":"phone","raw":"{\"t\":\"ble\",""" +
                """\"addr\":\"aa:aa:aa:00:00:01\",\"rssi\":-61,\"record\":\"020106\",\"ts_ns\":5}"}""",
            Feed.line(3, 7, raw),
        )
    }

    @Test
    fun aStoppedSessionEndsWithItsEndLine() {
        assertEquals("""{"type":"end","boot_ns":42}""", Feed.end(42))
    }

    @Test
    fun aWithheldLineSaysSo() {
        assertEquals(
            """{"type":"line","seq":1,"boot_ns":2,"source":"android","receiver":"phone","raw":"x","withheld":true}""",
            Feed.line(1, 2, "x", withheld = true),
        )
    }

    @Test
    fun aWifiScanKeepsAHiddenNameOutAndControlCharactersEscaped() {
        val raw = Feed.wifiRaw(
            listOf(
                Feed.WifiResult("cc:cc:cc:00:00:01", "a\"b\u0001", 2412, -50, "[WPA2-PSK-CCMP][ESS]", 9),
                Feed.WifiResult("cc:cc:cc:00:00:02", null, 5180, -70, null, 10),
            ),
        )
        assertEquals(
            """{"t":"wifi","results":[{"bssid":"cc:cc:cc:00:00:01","ssid":"a\"b\u0001","freq":2412,"level":-50,""" +
                """"caps":"[WPA2-PSK-CCMP][ESS]","ts_us":9},{"bssid":"cc:cc:cc:00:00:02","freq":5180,"level":-70,"ts_us":10}]}""",
            raw,
        )
    }

    @Test
    fun theSessionLinesNameTheSchemaAndTheClockBasis() {
        assertEquals("""{"type":"start","schema":"sensio.feed/1","app":"peil-android 0.1.0","boot_ns":1}""", Feed.start("peil-android 0.1.0", 1))
        assertEquals("""{"type":"clock","boot_ns":1,"utc_ns":2,"basis":"gnss"}""", Feed.clock(1, 2, gnss = true))
        assertEquals("""{"type":"dropped","source":"android","receiver":"phone","lines":4}""", Feed.dropped(4))
    }

    @Test
    fun aRadioLineNamesItsStateAsTheCoreDoes() {
        // The same text the core's live tests feed it.
        assertEquals(
            """{"type":"radio","boot_ns":10,"radio":"ble","receiver":"phone","state":"filtered"}""",
            Feed.radio(10, "ble", Feed.RadioState.FILTERED),
        )
        assertEquals(
            """{"type":"radio","boot_ns":20,"radio":"bt_classic","receiver":"phone","state":"unused"}""",
            Feed.radio(20, "bt_classic", Feed.RadioState.UNUSED),
        )
        assertEquals(
            """{"type":"radio","boot_ns":3,"radio":"link","receiver":"laptop","state":"off"}""",
            Feed.radio(3, "link", Feed.RadioState.OFF, receiver = "laptop"),
        )
    }

    @Test
    fun aLaptopLineCarriesItsSourceReceiverAndOwnStampAndItsOffsetLineThePair() {
        assertEquals(
            """{"type":"line","seq":5,"boot_ns":6,"source":"kismet","receiver":"laptop","remote_ns":7,"raw":"{}","withheld":true}""",
            Feed.line(5, 6, "{}", withheld = true, source = "kismet", receiver = "laptop", remoteNs = 7),
        )
        assertEquals(
            """{"type":"offset","receiver":"laptop","offset_ns":-12,"uncertainty_ns":3}""",
            Feed.offset("laptop", -12, 3),
        )
        assertEquals("""{"type":"dropped","source":"kismet","receiver":"laptop","lines":2}""", Feed.dropped(2, "kismet", "laptop"))
    }
}

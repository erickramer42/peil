package io.github.erickramer42.sensio

import java.io.ByteArrayInputStream
import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** The phone's side of `sensio.link/1`, against what the core's `link.rs` sends. */
class LinkProtocolTest {
    private fun refusal(type: String? = "hello", schema: String? = "sensio.link/1", code: String? = "123456", receiver: String? = "laptop") =
        LinkProtocol.refusal(type, schema, code, receiver, expected = "123456")

    @Test
    fun aHelloWithTheCodeAndAValidReceiverIsWelcomed() {
        assertNull(refusal())
    }

    @Test
    fun aHelloIsRefusedForTheWrongCodeSchemaTypeOrReceiver() {
        assertEquals(LinkProtocol.WRONG_CODE, refusal(code = "654321"))
        assertEquals(LinkProtocol.WRONG_CODE, refusal(code = null))
        assertEquals("this app speaks sensio.link/1", refusal(schema = "sensio.link/2"))
        assertEquals("expected a hello", refusal(type = "line"))
        for (receiver in listOf(null, "", "Laptop", "a b", "phone", "x".repeat(33))) {
            assertTrue("receiver $receiver", refusal(receiver = receiver)!!.startsWith("receiver must be"))
        }
    }

    @Test
    fun theOffsetTakesThePongAsStampedHalfwayThroughTheRoundTrip() {
        // Sent at boot 1000, back at 1400: the laptop's 50 was at boot 1200.
        assertEquals(Pair(1150L, 200L), LinkProtocol.offset(1_000, 1_400, 50))
        assertEquals(Pair(-1_800_000_000_000_000_000L + 2_000, 0L), LinkProtocol.offset(2_000, 2_000, 1_800_000_000_000_000_000))
    }

    @Test
    fun linesAreReadUpToTheirNewlineAndAnUnfinishedLastOneIsDropped() {
        val input = ByteArrayInputStream("""{"type":"pong"}""".toByteArray() + "\næ\nhalf".toByteArray(Charsets.UTF_8))
        assertEquals("""{"type":"pong"}""", LinkProtocol.readLine(input))
        assertEquals("æ", LinkProtocol.readLine(input))
        assertNull(LinkProtocol.readLine(input))
    }

    @Test
    fun aLineLongerThanTheLimitEndsTheConnection() {
        assertEquals("abcd", LinkProtocol.readLine(ByteArrayInputStream("abcd\n".toByteArray()), max = 4))
        assertThrows(IOException::class.java) { LinkProtocol.readLine(ByteArrayInputStream("abcde\n".toByteArray()), max = 4) }
    }

    @Test
    fun theTetherIsTheUsbInterfaceAndTheCodeHasSixDigits() {
        for (name in listOf("rndis0", "ncm0", "usb0")) assertTrue(name, LinkProtocol.isTether(name))
        for (name in listOf("wlan0", "rmnet_data0", "lo")) assertFalse(name, LinkProtocol.isTether(name))
        repeat(100) { assertTrue(LinkProtocol.newCode().matches(Regex("[0-9]{6}"))) }
    }

    @Test
    fun thePhonesLinesAreTheOnesTheLaptopReads() {
        assertEquals("""{"type":"welcome"}""", LinkProtocol.welcome())
        assertEquals("""{"type":"refused","reason":"wrong code"}""", LinkProtocol.refused(LinkProtocol.WRONG_CODE))
        assertEquals("""{"type":"ping","id":3}""", LinkProtocol.ping(3))
    }
}

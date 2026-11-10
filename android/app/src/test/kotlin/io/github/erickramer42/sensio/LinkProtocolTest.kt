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
    private fun refusal(type: String? = "hello", schema: String? = "sensio.link/1", receiver: String? = "laptop") =
        LinkProtocol.refusal(type, schema, receiver)

    /** The secret and nonce of the vector the core's link tests check too. */
    private val secret = ByteArray(32) { it.toByte() }
    private val nonce = "aa".repeat(16)

    @Test
    fun aHelloWithTheSchemaAndAValidReceiverGoesOn() {
        assertNull(refusal())
    }

    @Test
    fun aHelloIsRefusedForTheWrongSchemaTypeOrReceiver() {
        assertEquals("this app speaks sensio.link/1", refusal(schema = "sensio.link/2"))
        assertEquals("expected a hello", refusal(type = "line"))
        for (receiver in listOf(null, "", "Laptop", "a b", "phone", "x".repeat(33))) {
            assertTrue("receiver $receiver", refusal(receiver = receiver)!!.startsWith("receiver must be"))
        }
    }

    @Test
    fun theProofIsTheOneTheCoreComputes() {
        assertEquals(
            "d2fba96fe4aa315748030adf529ace1cdc3ecec2f9cf6be2cc8f46fc700a1dae",
            Feed.hex(LinkProtocol.proof(secret, "laptop", nonce)),
        )
    }

    @Test
    fun onlyTheRightProofForThisNonceAndReceiverIsAccepted() {
        val right = "d2fba96fe4aa315748030adf529ace1cdc3ecec2f9cf6be2cc8f46fc700a1dae"
        assertTrue(LinkProtocol.verify(secret, "laptop", nonce, right))
        assertTrue(LinkProtocol.verify(secret, "laptop", nonce, right.uppercase()))
        assertFalse(LinkProtocol.verify(secret, "laptop", "bb".repeat(16), right))
        assertFalse(LinkProtocol.verify(secret, "desktop", nonce, right))
        assertFalse(LinkProtocol.verify(ByteArray(32), "laptop", nonce, right))
        // The right bytes with a sign in place of a digit ("03" as "+3") are still refused.
        val signed = right.replaceRange(18, 20, "+3")
        for (bad in listOf(null, "", "zz", right.dropLast(1), right.dropLast(2), signed)) {
            assertFalse("$bad", LinkProtocol.verify(secret, "laptop", nonce, bad))
        }
    }

    @Test
    fun secretsAndNoncesAreFreshAndTheRightSize() {
        assertEquals(32, LinkProtocol.newSecret().size)
        assertTrue(LinkProtocol.newNonce().matches(Regex("[0-9a-f]{32}")))
        assertFalse(LinkProtocol.newSecret().contentEquals(LinkProtocol.newSecret()))
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
    fun ownAddressesKeepWellFormedEntriesLowercasedOnceEach() {
        // As the core's link tests write them.
        val own = LinkProtocol.own(
            listOf(
                "wifi" to "AA:BB:CC:DD:EE:FF",
                "bluetooth" to "aa:aa:aa:00:00:01",
                "wifi" to "aa:bb:cc:dd:ee:ff",
                "ble" to "aa:aa:aa:00:00:02",
                null to "aa:aa:aa:00:00:03",
                "wifi" to null,
                "wifi" to "aa:bb:cc:dd:ee",
                "wifi" to "aa:bb:cc:dd:ee:+f",
                "wifi" to "aabb.ccdd.eeff",
                "wifi" to "00:00:00:00:00:00",
            ),
        )
        assertEquals(listOf("wifi" to "aa:bb:cc:dd:ee:ff", "bluetooth" to "aa:aa:aa:00:00:01"), own)
        assertEquals(
            listOf("wifi:aa:bb:cc:dd:ee:ff", "bt:aa:aa:aa:00:00:01", "ble:aa:aa:aa:00:00:01"),
            LinkProtocol.ownDevices(own),
        )
    }

    @Test
    fun atMostSixteenOwnAddressesAreKept() {
        val many = (1..40).map { "wifi" to "aa:aa:aa:00:00:%02x".format(it) }
        assertEquals(LinkProtocol.MAX_OWN, LinkProtocol.own(many).size)
    }

    @Test
    fun thePhonesLinesAreTheOnesTheLaptopReads() {
        assertEquals("""{"type":"welcome"}""", LinkProtocol.welcome())
        assertEquals(
            """{"type":"welcome","secret":"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"}""",
            LinkProtocol.welcome(secret),
        )
        assertEquals("""{"type":"challenge","nonce":"$nonce"}""", LinkProtocol.challenge(nonce))
        assertEquals("""{"type":"refused","reason":"wrong code"}""", LinkProtocol.refused(LinkProtocol.WRONG_CODE))
        assertEquals("""{"type":"ping","id":3}""", LinkProtocol.ping(3))
    }
}

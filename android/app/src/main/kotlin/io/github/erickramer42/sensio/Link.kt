package io.github.erickramer42.sensio

import android.content.Context
import android.os.SystemClock
import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.concurrent.ConcurrentHashMap
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec
import org.json.JSONObject

/**
 * The link's rules that need no Android types, so the JVM unit tests cover
 * them: `sensio.link/1` (ADR-0004 section 6, amended 2026-11-07), the same
 * protocol the core's `link.rs` speaks from the laptop.
 */
object LinkProtocol {
    const val SCHEMA = "sensio.link/1"
    const val PORT = 8765
    const val WRONG_CODE = "wrong code"
    const val NOT_PAIRED = "not paired"

    /** The longest line either way, as the feed's (ADR-0005). */
    const val MAX_LINE_BYTES = 1 shl 20

    /** A new six-digit pairing code. */
    fun newCode(random: SecureRandom = SecureRandom()): String = "%06d".format(random.nextInt(1_000_000))

    /** Whether an interface is the USB tether's: `rndis0`, `ncm0` or `usb0`, as Android names them. */
    fun isTether(name: String): Boolean = listOf("rndis", "ncm", "usb").any { name.startsWith(it) }

    /** A name the events can carry, as the core's `is_label`: 1 to 32 of a-z, 0-9, '-', '_', '.'. */
    fun isLabel(s: String): Boolean =
        s.length in 1..32 && s.all { it in 'a'..'z' || it in '0'..'9' || it in "-_." }

    /** Why a hello is refused whatever it carries, or null to go on. */
    fun refusal(type: String?, schema: String?, receiver: String?): String? = when {
        type != "hello" -> "expected a hello"
        schema != SCHEMA -> "this app speaks $SCHEMA"
        receiver == null || !isLabel(receiver) || receiver == Feed.RECEIVER ->
            "receiver must be 1 to 32 characters of a-z, 0-9, '-', '_' or '.', and not ${Feed.RECEIVER}"
        else -> null
    }

    /** A new pairing's shared secret: 32 random bytes. */
    fun newSecret(random: SecureRandom = SecureRandom()): ByteArray = ByteArray(32).also { random.nextBytes(it) }

    /** A challenge's nonce: 16 random bytes, in hex. */
    fun newNonce(random: SecureRandom = SecureRandom()): String = Feed.hex(ByteArray(16).also { random.nextBytes(it) })

    /**
     * The proof a paired laptop must send for [nonce]: HMAC-SHA256 under the
     * secret of `sensio.link/1|<receiver>|<nonce>`, as the core's `link.rs`
     * computes it.
     */
    fun proof(secret: ByteArray, receiver: String, nonce: String): ByteArray =
        Mac.getInstance("HmacSHA256").run {
            init(SecretKeySpec(secret, "HmacSHA256"))
            doFinal("$SCHEMA|$receiver|$nonce".toByteArray(Charsets.UTF_8))
        }

    /** Whether [macHex] is the right proof, compared in constant time. */
    fun verify(secret: ByteArray, receiver: String, nonce: String, macHex: String?): Boolean {
        val mac = macHex?.let(::unhex) ?: return false
        return MessageDigest.isEqual(proof(secret, receiver, nonce), mac)
    }

    /** The bytes of lowercase or uppercase hex, or null. */
    fun unhex(text: String): ByteArray? {
        if (text.length % 2 != 0) return null
        return runCatching { ByteArray(text.length / 2) { text.substring(it * 2, it * 2 + 2).toInt(16).toByte() } }.getOrNull()
    }

    /**
     * The laptop's clock offset from one ping: this phone's boot clock minus
     * the laptop's clock, taking the pong as stamped halfway through the round
     * trip, and half the round trip as the uncertainty.
     */
    fun offset(sentNs: Long, receivedNs: Long, remoteNs: Long): Pair<Long, Long> {
        val half = (receivedNs - sentNs) / 2
        return Pair(sentNs + half - remoteNs, half)
    }

    /**
     * One line's bytes up to its newline, as UTF-8, or null at the end of the
     * stream (a last line without a newline is dropped: the sender stopped
     * mid-line). A line longer than [max] bytes ends the connection.
     */
    fun readLine(input: InputStream, max: Int = MAX_LINE_BYTES): String? {
        val out = ByteArrayOutputStream()
        while (true) {
            val b = input.read()
            if (b < 0) return null
            if (b == '\n'.code) return out.toString(Charsets.UTF_8.name())
            if (out.size() >= max) throw IOException("a line from the laptop is longer than $max bytes")
            out.write(b)
        }
    }

    fun welcome(secret: ByteArray? = null) =
        if (secret == null) """{"type":"welcome"}""" else """{"type":"welcome","secret":"${Feed.hex(secret)}"}"""

    fun challenge(nonce: String) = """{"type":"challenge","nonce":${Feed.quote(nonce)}}"""

    fun refused(reason: String) = """{"type":"refused","reason":${Feed.quote(reason)}}"""

    fun ping(id: Long) = """{"type":"ping","id":$id}"""
}

/**
 * The phone's side of the laptop link while a session runs and the user has
 * turned it on: a listener on the USB tether's address only, one laptop at a
 * time. A laptop pairs once with the code the screen shows (new each time
 * the link is turned on) and gets a shared secret; later it proves the
 * secret instead, so the link comes up with no one at the screen. After
 * three wrong codes, codes are refused until the link is turned off and on;
 * the paired laptop still gets in. The laptop's lines go to [line], its
 * measured clock offsets to [offset].
 */
class Link(
    private val context: Context,
    private val line: (source: String, receiver: String, remoteNs: Long, raw: String) -> Unit,
    private val offset: (receiver: String, offsetNs: Long, uncertaintyNs: Long) -> Unit,
) {
    private var code = ""
    private var on = false
    @Volatile private var running = true
    @Volatile private var server: ServerSocket? = null
    @Volatile private var client: Socket? = null
    private var wrong = 0
    private val thread = Thread(::serve, "link").apply {
        isDaemon = true
        start()
    }

    fun close() {
        running = false
        runCatching { server?.close() }
        runCatching { client?.close() }
        thread.join(2_000)
        Live.link(null)
    }

    private fun serve() {
        var bound: InetAddress? = null
        while (running) {
            val enabled = Options.link(context)
            if (enabled && !on) {
                code = LinkProtocol.newCode()
                wrong = 0
            }
            on = enabled
            val address = if (enabled) tetherAddress() else null
            if (address != bound) {
                runCatching { server?.close() }
                server = null
                bound = null
            }
            when {
                !enabled -> Live.link(null)
                address == null -> Live.link("Laptop: turn on USB tethering")
            }
            if (address == null) {
                SystemClock.sleep(IDLE_MS)
                continue
            }
            val listening = server ?: runCatching {
                ServerSocket(LinkProtocol.PORT, 1, address).apply { soTimeout = IDLE_MS.toInt() }
            }.onFailure {
                Live.link("Laptop: can't listen on ${address.hostAddress}:${LinkProtocol.PORT} (${it.javaClass.simpleName})")
                SystemClock.sleep(IDLE_MS)
            }.getOrNull() ?: continue
            server = listening
            bound = address
            Live.link(waiting(address))
            val socket = try {
                listening.accept()
            } catch (_: SocketTimeoutException) {
                continue
            } catch (_: IOException) {
                continue
            }
            client = socket
            val ended = runCatching { session(socket) }.exceptionOrNull()
            runCatching { socket.close() }
            client = null
            if (ended != null && running) {
                Live.link("Laptop: the link closed (${ended.message ?: ended.javaClass.simpleName})")
                SystemClock.sleep(IDLE_MS)
            }
        }
    }

    private fun session(socket: Socket) {
        socket.tcpNoDelay = true
        socket.soTimeout = HELLO_TIMEOUT_MS
        val input = BufferedInputStream(socket.getInputStream())
        val output = socket.getOutputStream()
        val hello = LinkProtocol.readLine(input)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: return
        val receiver = hello.text("receiver")
        val reason = LinkProtocol.refusal(hello.text("type"), hello.text("schema"), receiver)
        if (reason != null) {
            send(output, LinkProtocol.refused(reason))
            return
        }
        receiver!!
        val given = hello.text("code")
        if (given != null) {
            // Pairing: a right code gets a new secret, replacing any pairing before.
            val refusedCode = when {
                wrong >= MAX_WRONG -> "pairing is closed after $MAX_WRONG wrong codes; turn the link off and on"
                given != code -> LinkProtocol.WRONG_CODE
                else -> null
            }
            if (refusedCode != null) {
                if (refusedCode == LinkProtocol.WRONG_CODE) wrong += 1
                send(output, LinkProtocol.refused(refusedCode))
                return
            }
            val secret = LinkProtocol.newSecret()
            Pairing.save(context, receiver, secret)
            send(output, LinkProtocol.welcome(secret))
        } else {
            // A paired laptop proves the secret for a fresh nonce.
            val pairing = Pairing.get(context)
            if (pairing == null || pairing.first != receiver) {
                send(output, LinkProtocol.refused(LinkProtocol.NOT_PAIRED))
                return
            }
            val nonce = LinkProtocol.newNonce()
            send(output, LinkProtocol.challenge(nonce))
            val answer = LinkProtocol.readLine(input)?.let { runCatching { JSONObject(it) }.getOrNull() }
            val mac = answer?.takeIf { it.text("type") == "proof" }?.text("mac")
            if (!LinkProtocol.verify(pairing.second, receiver, nonce, mac)) {
                send(output, LinkProtocol.refused(LinkProtocol.NOT_PAIRED))
                return
            }
            send(output, LinkProtocol.welcome())
        }
        Live.link("Laptop: linked ($receiver)")
        socket.soTimeout = SILENT_MS

        val pings = ConcurrentHashMap<Long, Long>()
        val pinger = Thread({
            var id = 0L
            try {
                while (running && !socket.isClosed) {
                    id += 1
                    pings[id] = SystemClock.elapsedRealtimeNanos()
                    send(output, LinkProtocol.ping(id))
                    pings.keys.removeIf { it < id - 10 }
                    Thread.sleep(PING_EVERY_MS)
                }
            } catch (_: Exception) {
                runCatching { socket.close() }
            }
        }, "link-ping").apply {
            isDaemon = true
            start()
        }
        try {
            while (running && Options.link(context)) {
                val text = LinkProtocol.readLine(input) ?: return
                val o = runCatching { JSONObject(text) }.getOrNull() ?: continue
                when (o.text("type")) {
                    "pong" -> {
                        val received = SystemClock.elapsedRealtimeNanos()
                        val sent = pings.remove(o.optLong("id")) ?: continue
                        val remote = o.whole("remote_ns") ?: continue
                        if (received - sent > MAX_ROUND_TRIP_NS) continue
                        val (offsetNs, uncertaintyNs) = LinkProtocol.offset(sent, received, remote)
                        offset(receiver, offsetNs, uncertaintyNs)
                    }
                    "line" -> {
                        val source = o.text("source")?.takeIf(LinkProtocol::isLabel) ?: continue
                        val raw = o.text("raw") ?: continue
                        val remote = o.whole("remote_ns")?.takeIf { it >= 0 } ?: continue
                        line(source, receiver, remote, raw)
                    }
                    "status" -> o.text("text")?.take(MAX_STATUS)?.let { Live.link("Laptop ($receiver): $it") }
                }
            }
        } finally {
            runCatching { socket.close() }
            pinger.interrupt()
        }
    }

    private fun waiting(address: InetAddress): String = buildString {
        append("Laptop: waiting on ${address.hostAddress}:${LinkProtocol.PORT}")
        Pairing.get(context)?.let { append(" for ${it.first}") }
        if (wrong >= MAX_WRONG) {
            append("; pairing closed after $MAX_WRONG wrong codes (turn off and on)")
        } else {
            append("; code $code pairs a laptop")
        }
    }

    private fun send(output: OutputStream, text: String) = synchronized(output) {
        output.write((text + "\n").toByteArray(Charsets.UTF_8))
        output.flush()
    }

    /** The USB tether's IPv4 address, if tethering is on. */
    private fun tetherAddress(): InetAddress? = runCatching {
        NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()
            .filter { it.isUp && LinkProtocol.isTether(it.name) }
            .flatMap { it.inetAddresses.toList() }
            .firstOrNull { it is Inet4Address }
    }.getOrNull()

    private fun JSONObject.text(key: String): String? = if (has(key) && !isNull(key)) opt(key) as? String else null

    /** A whole number, never a rounded double. */
    private fun JSONObject.whole(key: String): Long? = when (val v = opt(key)) {
        is Long -> v
        is Int -> v.toLong()
        else -> null
    }

    companion object {
        const val MAX_WRONG = 3
        const val IDLE_MS = 2_000L
        const val HELLO_TIMEOUT_MS = 10_000
        const val PING_EVERY_MS = 5_000L

        /** Silence this long means the laptop is gone; it answers a ping every few seconds. */
        const val SILENT_MS = 30_000

        /** A round trip longer than this gives too loose an offset to use. */
        const val MAX_ROUND_TRIP_NS = 500_000_000L

        const val MAX_STATUS = 200
    }
}

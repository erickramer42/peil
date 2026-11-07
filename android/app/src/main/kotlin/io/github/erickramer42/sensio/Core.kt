package io.github.erickramer42.sensio

import java.io.BufferedWriter
import java.io.File
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

/**
 * The core as a process (ADR-0004 section 4): `peil live` reading the feed on
 * its stdin and writing the verdict stream on its stdout. The app never blocks
 * on it: [offer] refuses a line when the buffer is full, and the session
 * writes that line to its file as withheld (ADR-0005 section 5).
 */
class Core(binary: File, private val onLine: (String) -> Unit, private val onExit: (Int, String) -> Unit) {
    private val queue = ArrayBlockingQueue<String>(BUFFER_LINES)
    private val process: Process = ProcessBuilder(binary.path, "live").start()
    private val stdin: BufferedWriter = process.outputStream.bufferedWriter()
    @Volatile private var closing = false

    init {
        thread(name = "core-in") {
            try {
                while (true) {
                    val line = queue.poll(200, TimeUnit.MILLISECONDS)
                    if (line == null) {
                        stdin.flush()
                        if (closing) break
                        continue
                    }
                    if (line === END) break
                    stdin.write(line)
                    stdin.write('\n'.code)
                    if (queue.isEmpty()) stdin.flush()
                }
            } catch (_: Exception) {
                // The process ended; onExit says why.
            } finally {
                runCatching { stdin.close() }
            }
        }
        thread(name = "core-out") {
            process.inputStream.bufferedReader().forEachLine(onLine)
            val err = process.errorStream.bufferedReader().readText().trim().take(500)
            onExit(process.waitFor(), err)
        }
    }

    /** Passes a feed line on, or returns false when the buffer is full. */
    fun offer(line: String): Boolean = !closing && queue.offer(line)

    /** Room for at least [lines] more lines. */
    fun hasRoom(lines: Int): Boolean = queue.remainingCapacity() >= lines

    /** Ends the feed; the core writes its `end` line and exits. */
    fun close() {
        closing = true
        queue.offer(END)
    }

    /** Stops the core without waiting for its `end` line. */
    fun kill() {
        closing = true
        process.destroy()
    }

    companion object {
        /** About a minute of a busy road's lines. */
        const val BUFFER_LINES = 8192
        private val END = String(charArrayOf())
    }
}

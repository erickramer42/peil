package io.github.erickramer42.sensio

import android.app.Activity
import android.graphics.Typeface
import android.os.Handler
import android.os.Looper
import java.io.File
import kotlin.concurrent.thread

/**
 * A saved session's summary: the core's `peil summary` text report over its
 * feed, run as a process like the live core, so the phone shows what the
 * command gives over the same file.
 */
object SessionSummary {
    /** How many saved feeds the list offers, newest first. */
    const val LISTED = 30

    /** A feed's name as the list shows it: `2026-11-08_13-20-47.feed` reads `2026-11-08 13:20:47`. */
    fun label(name: String): String {
        val stem = name.removeSuffix(".feed")
        val parts = stem.split('_')
        if (parts.size != 2) return stem
        return "${parts[0]} ${parts[1].replace('-', ':')}"
    }

    /** The saved feeds, newest first. */
    fun newestFirst(files: List<File>): List<File> = files.filter { it.name.endsWith(".feed") }.sortedByDescending { it.name }

    /** Shows the summary of [feed] once the core has written it. */
    fun show(activity: Activity, feed: File) {
        val binary = Recordings.coreBinary(activity)
        val main = Handler(Looper.getMainLooper())
        thread(name = "summary") {
            val text = runCatching {
                val process = ProcessBuilder(binary.path, "summary", feed.path).start()
                val out = process.inputStream.bufferedReader().readText()
                val err = process.errorStream.bufferedReader().readText().trim().take(300)
                if (process.waitFor() == 0) out else "The core couldn't summarize this feed: $err"
            }.getOrElse { "The core couldn't run: ${it.message}" }
            main.post {
                if (activity.isFinishing || activity.isDestroyed) return@post
                val body = Ui.text(activity, 12f).apply {
                    this.text = text
                    typeface = Typeface.MONOSPACE
                    setTextIsSelectable(true)
                }
                Ui.dialog(activity, "Summary, ${label(feed.name)}", body)
            }
        }
    }

    /** The saved feeds to pick one from. */
    fun choose(activity: Activity) {
        val feeds = newestFirst(Recordings.files(activity))
        if (feeds.isEmpty()) {
            Ui.dialog(activity, "Summaries", Ui.text(activity, 15f).apply { text = "No saved feeds." })
            return
        }
        Ui.choices(activity, "Summaries", feeds.take(LISTED).map { f -> label(f.name) to { show(activity, f) } })
    }
}

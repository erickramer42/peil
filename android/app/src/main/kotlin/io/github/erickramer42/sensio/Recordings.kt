package io.github.erickramer42.sensio

import android.content.Context
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * The saved feeds, in the app's private storage (ADR-0004 section 7): every
 * raw line from the first session, kept for the retention period the user
 * sets and deleted all at once on request. They hold other people's
 * identifiers and the user's movements, so they never leave the phone from
 * here.
 */
object Recordings {
    private const val PREFS = "recordings"
    private const val RETENTION = "retention_days"
    val RETENTION_CHOICES = listOf(1, 7, 30)

    fun dir(context: Context): File = File(context.filesDir, "feeds").apply { mkdirs() }

    fun newFeed(context: Context): File =
        File(dir(context), SimpleDateFormat("yyyy-MM-dd_HH-mm-ss", Locale.US).format(Date()) + ".feed")

    fun files(context: Context): List<File> = dir(context).listFiles()?.filter { it.isFile }.orEmpty()

    fun retentionDays(context: Context): Int =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getInt(RETENTION, 7)

    fun setRetentionDays(context: Context, days: Int) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putInt(RETENTION, days).apply()

    /** Deletes the feeds older than the retention period. */
    fun prune(context: Context) {
        val cutoff = System.currentTimeMillis() - retentionDays(context) * 86_400_000L
        files(context).filter { it.lastModified() < cutoff }.forEach { it.delete() }
    }

    /** Deletes every saved feed; returns how many. */
    fun deleteAll(context: Context): Int = files(context).count { it.delete() }

    fun coreBinary(context: Context): File = File(context.applicationInfo.nativeLibraryDir, "libsensio.so")
}

/** The user's settings: what a session does, and how the screen shows it. */
object Options {
    private const val PREFS = "options"
    private const val CLASSIC = "classic_discovery"
    private const val LINK = "laptop_link"
    private const val RING_LARGE = "ring_large"

    /**
     * Bluetooth Classic discovery: off unless the user turns it on. It
     * transmits inquiries (ADR-0001 section 5), through the phone's own
     * certified radio, and slows BLE scanning while it runs.
     */
    fun classic(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(CLASSIC, false)

    fun setClassic(context: Context, on: Boolean) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(CLASSIC, on).apply()

    /** The laptop link (ADR-0004 section 6): off unless the user turns it on. */
    fun link(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(LINK, false)

    fun setLink(context: Context, on: Boolean) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(LINK, on).apply()

    /** The ring drawn large (the default) or small, leaving more room for the list. */
    fun ringLarge(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(RING_LARGE, true)

    fun setRingLarge(context: Context, large: Boolean) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(RING_LARGE, large).apply()
}

/**
 * The paired laptop (ADR-0004 section 6): its receiver name and the shared
 * secret, in the app's private storage, which backups leave out.
 */
object Pairing {
    private const val PREFS = "link_pairing"

    fun get(context: Context): Pair<String, ByteArray>? {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val receiver = prefs.getString("receiver", null) ?: return null
        val secret = prefs.getString("secret", null)?.let(LinkProtocol::unhex) ?: return null
        return Pair(receiver, secret)
    }

    fun save(context: Context, receiver: String, secret: ByteArray) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString("receiver", receiver).putString("secret", Feed.hex(secret)).commit()

    fun forget(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().clear().commit()
}

object BuildInfo {
    fun version(context: Context): String =
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: "unknown"
}

package io.github.erickramer42.sensio

import android.app.AlertDialog
import android.content.Context
import android.graphics.Typeface
import android.widget.ScrollView
import android.widget.TextView

/**
 * The About page (ADR-0004 section 8): the app's version and license, and
 * the licenses of the libraries packaged in the APK, from the APK's own
 * assets. Apache-2.0 asks that anyone who receives the program gets its
 * text. The list of libraries is the one `checkPackagedLibraries` allows.
 */
object About {
    const val MIT = "licenses/LICENSE-MIT"
    const val APACHE = "licenses/LICENSE-APACHE-2.0"

    fun text(context: Context): String {
        fun asset(path: String) = context.assets.open(path).bufferedReader().use { it.readText() }
        return buildString {
            append("Peil ${BuildInfo.version(context)}\n")
            append("What is transmitting around you, and is any of it unusual.\n\n")
            append("Peil is free software under the MIT License:\n\n")
            append(asset(MIT).trim())
            append("\n\n----\n\n")
            append("This app includes the Kotlin standard library (org.jetbrains.kotlin:kotlin-stdlib) ")
            append("and the JetBrains annotations (org.jetbrains:annotations), ")
            append("each under the Apache License, Version 2.0:\n\n")
            append(asset(APACHE).trim())
            append('\n')
        }
    }

    fun show(context: Context) {
        val body = TextView(context).apply {
            text = text(context)
            typeface = Typeface.MONOSPACE
            textSize = 11f
            setTextIsSelectable(true)
            val pad = (16 * resources.displayMetrics.density).toInt()
            setPadding(pad, pad, pad, pad)
        }
        AlertDialog.Builder(context)
            .setTitle("About Peil")
            .setView(ScrollView(context).apply { addView(body) })
            .setPositiveButton("Close", null)
            .show()
    }
}

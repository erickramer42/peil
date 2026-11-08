package io.github.erickramer42.sensio

import android.content.Context
import android.graphics.Typeface

/**
 * The About page (ADR-0004 section 8): the app's version, its two licenses
 * (MIT or Apache-2.0, at the recipient's option), and the libraries packaged
 * in the APK, under the same Apache-2.0 text, from the APK's own assets. Apache-2.0 asks that anyone who receives the program gets its
 * text. The list of libraries is the one `checkPackagedLibraries` allows.
 * The core is built from Rust crates used under MIT, whose notices go with
 * every copy; build-core.sh writes them into the assets beside the core.
 */
object About {
    const val MIT = "licenses/LICENSE-MIT"
    const val APACHE = "licenses/LICENSE-APACHE-2.0"
    const val CRATES = "licenses/RUST-CRATES"

    fun text(context: Context): String {
        fun asset(path: String) = context.assets.open(path).bufferedReader().use { it.readText() }
        return buildString {
            append("Peil ${BuildInfo.version(context)}\n")
            append("What is transmitting around you, and is any of it unusual.\n\n")
            append("Peil is free software, licensed under either of the Apache License, Version 2.0 ")
            append("or the MIT License, at your option.\n\n")
            append("This app also includes the Kotlin standard library (org.jetbrains.kotlin:kotlin-stdlib) ")
            append("and the JetBrains annotations (org.jetbrains:annotations), ")
            append("each under the Apache License, Version 2.0.\n\n")
            append("---- The MIT License ----\n\n")
            append(asset(MIT).trim())
            append("\n\n---- The Apache License, Version 2.0 ----\n\n")
            append(asset(APACHE).trim())
            append("\n\n---- The core's Rust crates ----\n\n")
            // A build without build-core.sh has no core either; say so rather than fail.
            append(runCatching { asset(CRATES).trim() }.getOrDefault("This build doesn't include the core."))
            append('\n')
        }
    }

    fun show(context: Context) {
        val body = Ui.text(context, 12f).apply {
            text = text(context)
            typeface = Typeface.MONOSPACE
            setTextIsSelectable(true)
        }
        Ui.dialog(context, "About Peil", body)
    }
}

package io.github.erickramer42.sensio

import java.io.File
import java.security.MessageDigest
import org.junit.Assert.assertEquals
import org.junit.Test

/** The license texts the About page shows are the right ones. */
class LicensesTest {
    private val assets = File("src/main/assets")

    @Test
    fun theAppsMitLicenseIsTheRepositorys() {
        assertEquals(File("../../LICENSE-MIT").readText(), File(assets, About.MIT).readText())
    }

    @Test
    fun theApacheLicenseIsTheCanonicalText() {
        // The SHA-256 of https://www.apache.org/licenses/LICENSE-2.0.txt.
        val sha = MessageDigest.getInstance("SHA-256").digest(File(assets, About.APACHE).readBytes())
            .joinToString("") { "%02x".format(it) }
        assertEquals("cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30", sha)
    }
}

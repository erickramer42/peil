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
    fun theAppsApacheLicenseIsTheRepositorys() {
        assertEquals(File("../../LICENSE-APACHE").readText(), File(assets, About.APACHE).readText())
    }

    @Test
    fun theApacheLicenseIsTheCanonicalText() {
        // The SHA-256 of https://www.apache.org/licenses/LICENSE-2.0.txt.
        val sha = MessageDigest.getInstance("SHA-256").digest(File(assets, About.APACHE).readBytes())
            .joinToString("") { "%02x".format(it) }
        assertEquals("cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30", sha)
    }

    @Test
    fun theFontsAreB612sOwnFilesAndTheirLicenseShipsWithThem() {
        // SHA-256 of github.com/polarsys/b612's fonts/ttf files and OFL.txt (with LF line ends), read 2026-11-09.
        fun sha(bytes: ByteArray) = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
        val fonts = File("src/main/res/font").listFiles { f -> f.extension == "ttf" }.orEmpty()
        assertEquals(
            mapOf(
                "b612_bold" to "91749541ac7b2c328b58832b7e2c4df809d7e2ba38d62a3a5aa3f8e38b271814",
                "b612_mono_bold" to "b467b1d19fdabed42be51d87e38c86645ceeff2f828f294775188d00d1fd68ca",
                "b612_mono_regular" to "b98cb96cc8a6206dae08c063d60902df7e6d40f86139ebdb97256704253c9c69",
                "b612_regular" to "139dce659100a83bf95b48474696e448bee95631ef84fd3d0437ced2bf33cf73",
            ),
            fonts.associate { it.nameWithoutExtension to sha(it.readBytes()) },
        )
        val ofl = File(assets, About.OFL).readText().replace("\r\n", "\n").toByteArray()
        assertEquals("41eb0cf56ee1bae213d4a5c650fe60d936bdeda605d3cc6abd7a1720f6083e62", sha(ofl))
    }
}

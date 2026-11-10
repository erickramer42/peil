package io.github.erickramer42.sensio

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Test

/** The saved-feed list behind the summary view. */
class SessionSummaryTest {
    @Test
    fun aFeedsNameReadsAsItsStartTime() {
        assertEquals("2026-11-08 13:20:47", SessionSummary.label("2026-11-08_13-20-47.feed"))
        assertEquals("odd-name", SessionSummary.label("odd-name.feed"))
    }

    @Test
    fun feedsListNewestFirstAndOtherFilesStayOut() {
        val files = listOf("2026-11-07_09-00-00.feed", "notes.txt", "2026-11-08_13-20-47.feed", "2026-11-08_09-05-43.feed").map(::File)
        assertEquals(
            listOf("2026-11-08_13-20-47.feed", "2026-11-08_09-05-43.feed", "2026-11-07_09-00-00.feed"),
            SessionSummary.newestFirst(files).map { it.name },
        )
    }
}

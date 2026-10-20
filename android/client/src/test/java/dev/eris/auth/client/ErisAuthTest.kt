package dev.eris.auth.client

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ErisAuthTest {
    @Test
    fun anApprovalRequestIsTheAgentSocketsLine() {
        val line = JSONObject(ErisAuth.approveJson("erisdb", "Tasks", listOf("tasks:read", "tasks:write"), false, "ab".repeat(32)))
        assertEquals("approve", line.getString("op"))
        assertEquals(2, line.getInt("v"))
        assertEquals("erisdb", line.getString("service"))
        assertEquals("Tasks", line.getString("name"))
        assertEquals(listOf("tasks:read", "tasks:write"), List(2) { line.getJSONArray("grants").getString(it) })
        assertEquals(false, line.getBoolean("serves"))
        assertEquals("ab".repeat(32), line.getString("app"))
        assertEquals(setOf("op", "v", "service", "name", "grants", "serves", "app"), line.keys().asSequence().toSet())
    }

    @Test
    fun aSnapshotRequestIsTheAgentSocketsLine() {
        val line = JSONObject(ErisAuth.SNAPSHOT)
        assertEquals("snapshot", line.getString("op"))
        assertEquals(2, line.getInt("v"))
        assertEquals(setOf("op", "v"), line.keys().asSequence().toSet())
    }

    @Test
    fun anApprovalCarriesTheNoteAndTheSnapshotAsJson() {
        val reply = """{"approved":{"note":{"by":"ab","body":"eyJ9","sig":"cd"},"snapshot":{"mesh":"ef","entries":[],"directory":[]}}}"""
        val answer = ErisAuth.Answer.parse(reply) as ErisAuth.Answer.Approved
        val note = JSONObject(answer.note)
        assertEquals("ab", note.getString("by"))
        assertEquals("eyJ9", note.getString("body"))
        assertEquals("cd", note.getString("sig"))
        val snapshot = JSONObject(answer.snapshot)
        assertEquals("ef", snapshot.getString("mesh"))
        assertEquals(0, snapshot.getJSONArray("entries").length())
    }

    @Test
    fun denialsAndErrorsAreAnswersToo() {
        assertEquals(ErisAuth.Answer.Denied("the person said no"), ErisAuth.Answer.parse("""{"denied":"the person said no"}"""))
        assertEquals(
            ErisAuth.Answer.Failed("this device is in no mesh yet"),
            ErisAuth.Answer.parse("""{"error":"this device is in no mesh yet"}"""),
        )
        assertEquals(ErisAuth.Answer.Failed("ErisAuth sent something unreadable"), ErisAuth.Answer.parse("nonsense"))
        assertTrue(ErisAuth.Answer.parse("""{"snapshot":{}}""") is ErisAuth.Answer.Failed)
    }
}

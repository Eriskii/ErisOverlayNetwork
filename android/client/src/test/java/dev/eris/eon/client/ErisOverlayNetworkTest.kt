package dev.eris.eon.client

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ErisOverlayNetworkTest {
    @Test
    fun anApprovalRequestIsTheAgentSocketsLine() {
        val line = JSONObject(
            ErisOverlayNetwork.approveJson(
                "Tasks",
                mapOf("database/1" to listOf("tasks:read", "tasks:write")),
                emptyList(),
                "ab".repeat(32),
            ),
        )
        assertEquals("approve", line.getString("op"))
        assertEquals(3, line.getInt("v"))
        assertEquals("Tasks", line.getString("name"))
        val uses = line.getJSONObject("uses")
        assertEquals(setOf("database/1"), uses.keys().asSequence().toSet())
        assertEquals(listOf("tasks:read", "tasks:write"), List(2) { uses.getJSONArray("database/1").getString(it) })
        assertEquals(0, line.getJSONArray("serves").length())
        assertEquals("ab".repeat(32), line.getString("app"))
        assertEquals(setOf("op", "v", "name", "uses", "serves", "app"), line.keys().asSequence().toSet())
    }

    @Test
    fun anAppThatServesSaysHow() {
        val line = JSONObject(
            ErisOverlayNetwork.approveJson(
                "ErisFiles",
                mapOf("files/1" to listOf("files:read")),
                listOf(ErisOverlayNetwork.Serves("files/1"), ErisOverlayNetwork.Serves("database/1", single = true, passphrase = true)),
                "cd".repeat(32),
            ),
        )
        val serves = line.getJSONArray("serves")
        assertEquals("files/1", serves.getJSONObject(0).getString("capability"))
        assertEquals(setOf("capability"), serves.getJSONObject(0).keys().asSequence().toSet())
        assertEquals(true, serves.getJSONObject(1).getBoolean("single"))
        assertEquals(true, serves.getJSONObject(1).getBoolean("passphrase"))
    }

    @Test
    fun aSnapshotRequestIsTheAgentSocketsLine() {
        val line = JSONObject(ErisOverlayNetwork.SNAPSHOT)
        assertEquals("snapshot", line.getString("op"))
        assertEquals(3, line.getInt("v"))
        assertEquals(setOf("op", "v"), line.keys().asSequence().toSet())
    }

    @Test
    fun anApprovalCarriesTheNoteAndTheSnapshotAsJson() {
        val reply = """{"approved":{"note":{"by":"ab","body":"eyJ9","sig":"cd"},"snapshot":{"mesh":"ef","entries":[],"directory":[]}}}"""
        val answer = ErisOverlayNetwork.Answer.parse(reply) as ErisOverlayNetwork.Answer.Approved
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
        assertEquals(ErisOverlayNetwork.Answer.Denied("the person said no"), ErisOverlayNetwork.Answer.parse("""{"denied":"the person said no"}"""))
        assertEquals(
            ErisOverlayNetwork.Answer.Failed("this device is in no mesh yet"),
            ErisOverlayNetwork.Answer.parse("""{"error":"this device is in no mesh yet"}"""),
        )
        assertEquals(ErisOverlayNetwork.Answer.Failed("ErisOverlayNetwork sent something unreadable"), ErisOverlayNetwork.Answer.parse("nonsense"))
        assertTrue(ErisOverlayNetwork.Answer.parse("""{"snapshot":{}}""") is ErisOverlayNetwork.Answer.Failed)
    }
}

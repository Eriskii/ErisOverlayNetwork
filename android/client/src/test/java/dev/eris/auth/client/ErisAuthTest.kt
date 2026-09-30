package dev.eris.auth.client

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class ErisAuthTest {
    @Test
    fun anAskIsErisLoginsAskInJson() {
        val ask = JSONObject(
            ErisAuth.askJson("erisdb", "Notes", listOf("notes:read", "notes:create"), ErisAuth.Identity.iroh("ab".repeat(32))),
        )
        assertEquals("erisdb", ask.getString("service"))
        assertEquals("Notes", ask.getString("client"))
        assertEquals(listOf("notes:read", "notes:create"), List(2) { ask.getJSONArray("requested").getString(it) })
        assertEquals("iroh", ask.getJSONObject("identity").getString("kind"))
        assertEquals("ab".repeat(32), ask.getJSONObject("identity").getString("key"))
    }

    @Test
    fun answersAreApprovalsDenialsOrFailures() {
        assertEquals(ErisAuth.Answer.Approved(listOf("notes:read")), ErisAuth.Answer.parse("""{"approved":["notes:read"]}"""))
        assertEquals(ErisAuth.Answer.Denied("the person said no"), ErisAuth.Answer.parse("""{"denied":"the person said no"}"""))
        assertEquals(ErisAuth.Answer.Failed("not joined"), ErisAuth.Answer.parse("""{"error":"not joined"}"""))
        assertEquals(ErisAuth.Answer.Failed("ErisAuth sent something unreadable"), ErisAuth.Answer.parse("nonsense"))
    }
}

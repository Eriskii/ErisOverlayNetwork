package dev.eris.auth

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ModelTest {
    @Test
    fun grantsMatchAsErisLoginMatchesThem() {
        assertTrue(covers("tasks:*", "tasks:create"))
        assertFalse(covers("tasks:read", "tasks:create"))
        assertTrue(covers("*", "meta:pairing:approve"))
        assertFalse(covers("*:read", "meta:facets:read"))
        assertTrue(covers("*:read", "notes:read"))
        assertFalse(covers("tasks:read", "tasks:*"))
        assertTrue(encloses(listOf("notes:*"), listOf("notes:read", "notes:delete")))
        assertFalse(encloses(listOf("notes:read"), listOf("notes:read", "notes:delete")))
        assertTrue(needsPassphrase(listOf("notes:read"), listOf("tasks:read")))
        assertFalse(needsPassphrase(listOf("*"), listOf("tasks:read")))
    }

    @Test
    fun aCeilingAsTypedIsGrantsAndNothingTypedIsEverything() {
        assertEquals(listOf("tasks:*", "lists:*"), parseCeiling(" tasks:*, lists:* "))
        assertEquals(listOf("*"), parseCeiling("   "))
    }

    @Test
    fun timeLeftReadsAsMinutesAndSeconds() {
        assertEquals("2:05", remaining(125, 0))
        assertEquals("0:00", remaining(10, 20))
    }

    @Test
    fun theStateReadsWhatTheDaemonSays() {
        val state = DeviceState.parse(
            JSONObject(
                """{"device":{"name":"Phone","joined":true,"grants":["meta:pairing:create","erisauth:approve","tasks:*"]},
                   "approvals":[{"id":"a1","kind":"app","service":"erisdb","client":"Tasks","requested":["tasks:read","lists:read"],
                     "identity":{"kind":"iroh","key":"ab"},"fingerprint":null,"essential":[],"pid":null,"executable":null,
                     "package":"dev.erisdb.tasks","certificate":"cd","needs_passphrase":true,"expires":100}],
                   "joining":null}""",
            ),
        )
        assertEquals("Phone", state.name)
        assertTrue(state.joined)
        assertEquals("what it approves by itself, without its fixed rights", listOf("tasks:*"), state.approvesItself)
        val approval = state.approvals.single()
        assertEquals("Tasks", approval.client)
        assertEquals("dev.erisdb.tasks", approval.packageName)
        assertEquals(listOf("tasks:read", "lists:read"), approval.optional)
        assertNull(state.joining)
    }

    @Test
    fun aDeviceJoiningIsGrantedItsFixedRightsWholeAndTheRestCanBeNarrowed() {
        val approval = Approval.parse(
            JSONObject(
                """{"id":"d1","kind":"device","service":"erisdb","client":"ErisAuth on Laptop",
                   "requested":["meta:pairing:create","erisauth:approve","notes:*"],"identity":null,"fingerprint":"1234-5678",
                   "essential":["meta:pairing:create","erisauth:approve"],"pid":null,"executable":null,"package":null,
                   "certificate":null,"needs_passphrase":false,"expires":100}""",
            ),
        )
        assertTrue(approval.isDevice)
        assertEquals(listOf("notes:*"), approval.optional)
        assertEquals(listOf("meta:pairing:create", "erisauth:approve"), approval.granted(emptySet()))
        assertEquals("Approve", approveLabel(picked = 1, of = 1))
        assertEquals("Approve 1 of 3", approveLabel(picked = 1, of = 3))
    }

    @Test
    fun aQrCodeIsRowsOfModules() {
        val qr = Qr.parse(JSONObject("""{"side":3,"modules":"010111000"}"""))
        assertEquals(3, qr.side)
        assertTrue(qr.dark(1, 0))
        assertFalse(qr.dark(0, 0))
        assertTrue(qr.dark(2, 1))
        assertFalse(qr.dark(2, 2))
    }

    @Test
    fun passphraseAnswersSayWhatWentWrongInWords() {
        assertEquals("", passphraseProblem(JSONObject("""{"ok":true}"""), PassphraseAction.CHANGE))
        assertEquals("That is not the current passphrase.", passphraseProblem(JSONObject("""{"error":"wrong_passphrase"}"""), PassphraseAction.CHANGE))
        assertEquals(
            "This device may not set the passphrase without the current one.",
            passphraseProblem(JSONObject("""{"error":"forbidden"}"""), PassphraseAction.RESET),
        )
        assertEquals("This device may not unlock devices.", passphraseProblem(JSONObject("""{"error":"forbidden"}"""), PassphraseAction.UNLOCK))
        assertEquals(
            "Too many wrong passphrases from this device; it may try again in 2:05.",
            passphraseProblem(JSONObject("""{"error":"locked","retry_after":125}"""), PassphraseAction.CHANGE),
        )
        assertEquals(
            "There is no passphrase yet; leave the current one empty to set the first.",
            passphraseProblem(JSONObject("""{"error":"no_passphrase"}"""), PassphraseAction.CHANGE),
        )
        assertEquals("ErisDB is not answering", passphraseProblem(JSONObject("""{"error":"unreachable","message":"ErisDB is not answering"}"""), PassphraseAction.RESET))
    }

    @Test
    fun anApprovalRefusalSaysWhyInWords() {
        assertEquals("That is not the passphrase.", refusalOf(JSONObject("""{"error":"wrong_passphrase"}""")))
        assertEquals(
            "Too many wrong passphrases from this device. Try again in 1:00.",
            refusalOf(JSONObject("""{"error":"locked","retry_after":60}""")),
        )
        assertEquals("ErisDB has no ErisAuth plugin", refusalOf(JSONObject("""{"error":"not_set_up","message":"ErisDB has no ErisAuth plugin"}""")))
    }
}

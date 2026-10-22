package dev.eris.eon

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class CallerTest {
    @Test
    fun aCertificateIsNamedByTheHexOfItsSha256() {
        // SHA-256 of the three bytes "abc".
        assertEquals(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            Caller.fingerprint("abc".toByteArray()),
        )
    }

    @Test
    fun theCallerIsWhatAndroidSaysNotWhatTheAppClaims() {
        val caller = JSONObject(Caller("dev.example.notes", "ab".repeat(32), 4242).json())
        assertEquals("dev.example.notes", caller.getString("package"))
        assertEquals("ab".repeat(32), caller.getString("certificate"))
        assertEquals(4242, caller.getInt("pid"))
    }
}

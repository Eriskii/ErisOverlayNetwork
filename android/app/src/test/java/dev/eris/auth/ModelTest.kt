package dev.eris.auth

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ModelTest {
    private val state = State.parse(
        JSONObject(
            """{"version":7,"device":{"id":"d1","name":"phone"},
               "mesh":{"id":"m1","member":true,
                 "devices":[{"id":"d0","name":"laptop","added_by":null,"added_by_name":null,"added_at":100,"this":false,"reachable":true},
                            {"id":"d1","name":"phone","added_by":"d0","added_by_name":"laptop","added_at":200,"this":true,"reachable":true},
                            {"id":"d2","name":"tablet","added_by":"d1","added_by_name":"phone","added_at":300,"this":false,"reachable":null}],
                 "erisdb":{"app":"e1","note":"n0","name":"ErisDB","device":"laptop","approved_at":40,"reachable":true}},
               "joining":null,
               "approvals":[
                 {"id":"a1","kind":"app","via":"agent","name":"Tasks","key":"k1","service":"erisdb","grants":["tasks:read","tasks:write"],
                  "serves":false,"program":"dev.example.tasks sha256:cdcd","pid":4242,"package":"dev.example.tasks","certificate":"cdcd",
                  "fingerprint":null,"passphrase":true,"becomes_erisdb":false,"expires":1000,"busy":false},
                 {"id":"a2","kind":"device","via":"ticket","name":"watch","key":"k2","service":null,"grants":[],"serves":false,
                  "program":null,"pid":null,"package":null,"certificate":null,"fingerprint":"12AB-34CD-56EF","passphrase":false,
                  "becomes_erisdb":false,"expires":900,"busy":false},
                 {"id":"a3","kind":"app","via":"ticket","name":"ErisDB 2","key":"k3","service":"erisdb","grants":[],"serves":true,
                  "program":null,"pid":null,"package":null,"certificate":null,"fingerprint":"AAAA-BBBB-CCCC","passphrase":false,
                  "becomes_erisdb":true,"expires":800,"busy":false}],
               "tickets":[{"id":"t1","purpose":"device","status":"pending","expires":700}],
               "notes":[{"id":"n1","app":"k9","name":"ErisFiles","service":"files","grants":["files:read"],"serves":true,
                         "program":"dev.example.files sha256:abab","at":50,"revoked":true,"counts":false,"erisdb":false}],
               "events":[{"seq":1,"at":60,"text":"laptop added tablet"}]}""",
        ),
    )

    @Test
    fun theStateReadsWhatTheDaemonSays() {
        assertEquals(7, state.version)
        assertEquals("phone", state.deviceName)
        val mesh = state.mesh!!
        assertTrue(mesh.member)
        assertEquals(listOf("laptop", "phone", "tablet"), mesh.devices.map { it.name })
        val phone = mesh.devices[1]
        assertTrue(phone.isThis)
        assertEquals("laptop", phone.addedByName)
        assertNull("the founder was added by no one", mesh.devices[0].addedBy)
        assertNull("not asked yet", mesh.devices[2].reachable)
        assertEquals("ErisDB", mesh.erisdb!!.name)
        assertNull(state.joining)
        assertEquals("t1", state.tickets.single().id)
        assertEquals("laptop added tablet", state.events.single().text)
        val note = state.notes.single()
        assertEquals("revoked", note.standing)
        assertEquals(listOf("files:read"), note.grants)
    }

    @Test
    fun aMeshWithNoErisDbAndADeviceWithNoMesh() {
        val lonely = State.parse(JSONObject("""{"version":1,"device":{"id":"d1","name":null},"mesh":null,"joining":
            {"fingerprint":"12AB-34CD-56EF","host":"laptop","error":null},"approvals":[],"tickets":[],"notes":[],"events":[]}"""))
        assertNull(lonely.mesh)
        assertNull(lonely.deviceName)
        assertEquals(Joining("12AB-34CD-56EF", "laptop", null), lonely.joining)
        val alone = Mesh.parse(JSONObject("""{"id":"m","member":false,"devices":[],"erisdb":null}"""))
        assertFalse(alone.member)
        assertNull(alone.erisdb)
    }

    @Test
    fun eachRequestSaysWhatItAsksAndWho() {
        val (tasks, watch, server) = state.approvals
        assertEquals("asks to use ErisDB", tasks.asks)
        assertEquals("dev.example.tasks", tasks.who)
        assertTrue(tasks.passphrase)
        assertEquals("asks to join your devices", watch.asks)
        assertTrue(watch.isDevice && watch.viaTicket)
        assertEquals("asks to be your ErisDB", server.asks)
        assertEquals("An app with nothing on this phone to vouch for it", server.who)
        assertEquals("Tasks asks to use ErisDB" to "dev.example.tasks wants tasks:read, tasks:write.", tasks.notification())
        assertEquals(
            "watch asks to join your devices" to "Compare the code 12AB-34CD-56EF with the new device's screen.",
            watch.notification(),
        )
        assertEquals("ErisDB 2 asks to be your ErisDB" to "An app with nothing on this phone to vouch for it wants nothing more.", server.notification())
    }

    @Test
    fun approvingSendsTheChosenGrantsInTheOrderAskedAndThePassphrase() {
        val (tasks, watch) = state.approvals
        val decide = tasks.decide(setOf("tasks:write", "tasks:read"), "correct horse")
        assertEquals("decide", decide.getString("op"))
        assertEquals("a1", decide.getString("id"))
        assertEquals(listOf("tasks:read", "tasks:write"), List(2) { decide.getJSONArray("grants").getString(it) })
        assertEquals("correct horse", decide.getString("passphrase"))
        assertEquals(1, tasks.decide(setOf("tasks:read"), "x").getJSONArray("grants").length())
        val device = watch.decide(emptySet(), "")
        assertFalse("a device is approved as it asked", device.has("grants"))
        assertFalse(device.has("passphrase"))
    }

    @Test
    fun aRemovalCarriesTheDeviceAndTheDevicesItAddedThatStayTicked() {
        val removal = Removal.parse(
            JSONObject("""{"device":{"id":"d1","name":"phone"},"also":[{"id":"d2","name":"tablet"},{"id":"d3","name":"watch"}]}"""),
        )
        assertEquals(listOf("tablet", "watch"), removal.also.map { it.name })
        assertEquals(listOf("d1", "d2", "d3"), removal.devices(emptySet()))
        assertEquals(listOf("d1", "d2"), removal.devices(setOf("d3")))
    }

    @Test
    fun aTicketComesWithItsQrCodeLaidOutInModules() {
        val opened = Opened.parse(JSONObject("""{"id":"t1","ticket":"eris://pair/x","qr":{"side":3,"modules":"010111000"},"expires":600}"""))
        assertEquals("eris://pair/x", opened.ticket)
        assertEquals(3, opened.qr.side)
        assertTrue(opened.qr.dark(1, 0))
        assertFalse(opened.qr.dark(0, 0))
        assertTrue(opened.qr.dark(2, 1))
        assertFalse(opened.qr.dark(2, 2))
    }

    @Test
    fun timeReadsAsAPersonSaysIt() {
        assertEquals("2:05", remaining(125, 0))
        assertEquals("0:00", remaining(10, 20))
        assertEquals("just now", ago(100, 130))
        assertEquals("1 minute ago", ago(0, 60))
        assertEquals("3 hours ago", ago(0, 3 * 3600 + 5))
        assertEquals("2 days ago", ago(0, 2 * 86_400))
        assertEquals("abcdefgh…uvwxyz", short("abcdefghijklmnopqrstuvwxyz"))
        assertEquals("short", short("short"))
        assertEquals("Approve", approveLabel(picked = 2, of = 2))
        assertEquals("Approve 1 of 2", approveLabel(picked = 1, of = 2))
    }

    @Test
    fun refusalsSayWhatWentWrongInWords() {
        assertNull(refusal(JSONObject("""{"ok":true}""")))
        assertEquals("This takes the ErisDB passphrase.", refusal(JSONObject("""{"error":"needs_passphrase","message":"x"}""")))
        assertEquals("That is not the passphrase.", refusal(JSONObject("""{"error":"wrong_passphrase"}""")))
        assertEquals(
            "Too many wrong passphrases from this phone. Try again in 1:00.",
            refusal(JSONObject("""{"error":"locked","retry_after":60,"message":"x"}""")),
        )
        assertEquals("Approve only what was asked for.", refusal(JSONObject("""{"error":"not_asked"}""")))
        assertEquals(
            "this mesh has no ErisDB yet; run `erisdb join` where ErisDB runs",
            refusal(JSONObject("""{"error":"no_erisdb","message":"this mesh has no ErisDB yet; run `erisdb join` where ErisDB runs"}""")),
        )
        assertEquals("That is not the current passphrase.", passphraseProblem(JSONObject("""{"error":"wrong_passphrase"}""")))
        assertNull(passphraseProblem(JSONObject("""{"ok":true}""")))
        assertEquals("At least 8 characters.", newPassphraseProblem("short", ""))
        assertEquals("The two differ.", newPassphraseProblem("long enough", "long enougj"))
        assertNull(newPassphraseProblem("long enough", "long enough"))
        assertEquals("ErisDB", serviceName("erisdb"))
        assertEquals("files", serviceName("files"))
    }
}

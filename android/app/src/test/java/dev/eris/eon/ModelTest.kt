package dev.eris.eon

import java.util.Base64
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ModelTest {
    private val state = State.parse(
        JSONObject(
            """{"version":7,"device":{"id":"d1","name":"phone","hostname":"localhost"},
               "mesh":{"id":"m1","member":true,
                 "devices":[{"id":"d0","name":"laptop","added_by":null,"added_by_name":null,"added_at":100,"this":false,"reachable":true},
                            {"id":"d1","name":"phone","added_by":"d0","added_by_name":"laptop","added_at":200,"this":true,"reachable":true},
                            {"id":"d2","name":"tablet","added_by":"d1","added_by_name":"phone","added_at":300,"this":false,"reachable":null}],
                 "servers":[{"capability":"database/1","single":true,"passphrase":true,
                             "apps":[{"app":"e2","note":"n2","name":"ErisDB","device":"laptop","approved_at":40,"reachable":true},
                                     {"app":"e1","note":"n0","name":"Old ErisDB","device":"laptop","approved_at":20,"reachable":null}]},
                            {"capability":"files/1","single":false,"passphrase":false,
                             "apps":[{"app":"f1","note":"n3","name":"ErisFiles","device":"tablet","approved_at":30,"reachable":null}]}]},
               "joining":null,
               "offer":null,
               "inviting":{"name":"mac","status":"waiting","error":null},
               "approvals":[
                 {"id":"a1","kind":"app","via":"agent","name":"Tasks","key":"k1",
                  "uses":{"database/1":["tasks:read","tasks:write"],"files/1":["files:read"]},"serves":[],
                  "program":"dev.example.tasks sha256:cdcd","pid":4242,"package":"dev.example.tasks","certificate":"cdcd",
                  "fingerprint":null,"passphrase":true,"replaces":[],"expires":1000,"busy":false},
                 {"id":"a2","kind":"device","via":"ticket","name":"watch","key":"k2","uses":{},"serves":[],
                  "program":null,"pid":null,"package":null,"certificate":null,"fingerprint":"12AB-34CD-56EF","passphrase":false,
                  "replaces":[],"expires":900,"busy":false},
                 {"id":"a3","kind":"app","via":"ticket","name":"ErisDB 2","key":"k3","uses":{},
                  "serves":[{"capability":"database/1","single":true,"passphrase":true}],
                  "program":null,"pid":null,"package":null,"certificate":null,"fingerprint":"AAAA-BBBB-CCCC","passphrase":false,
                  "replaces":[{"capability":"database/1","name":"ErisDB"}],"expires":800,"busy":false}],
               "tickets":[{"id":"t1","purpose":"device","status":"pending","expires":700}],
               "notes":[{"id":"n1","app":"k9","name":"ErisFiles","uses":{"files/1":["files:read"]},
                         "serves":[{"capability":"files/1"}],
                         "program":"dev.example.files sha256:abab","at":50,"revoked":true,"counts":false}],
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
        assertNull(state.joining)
        assertEquals("t1", state.tickets.single().id)
        assertEquals("laptop added tablet", state.events.single().text)
        val note = state.notes.single()
        assertEquals("revoked", note.standing)
        assertEquals(mapOf("files/1" to listOf("files:read")), note.uses)
        assertEquals("uses files/1, serves files/1", note.approves)
        assertEquals(Inviting("mac", "waiting", null), state.inviting)
    }

    @Test
    fun whatTheAppsServeShowsTheOneInUseOfASingleServer() {
        val (database, files) = state.mesh!!.servers
        assertEquals("your database", your(database.capability))
        assertTrue(database.single && database.passphrase)
        assertEquals(listOf("ErisDB"), database.inUse.map { it.name })
        assertEquals(listOf("ErisFiles"), files.inUse.map { it.name })
        assertEquals(listOf("database/1"), state.mesh!!.passphrases)
    }

    @Test
    fun aDeviceInNoMeshShowsItsOwnTicketAndHearsTheOffer() {
        val lonely = State.parse(
            JSONObject(
                """{"version":1,"device":{"id":"d1","name":null,"hostname":"pixel"},"mesh":null,"joining":
                {"fingerprint":"12AB-34CD-56EF","host":"laptop","error":null},
                "offer":{"name":"pixel","ticket":"eris://join/x","qr":{"side":1,"modules":"1"},"expires":600,
                         "status":"offered","by":"laptop","devices":["desktop","laptop"]},
                "inviting":null,"approvals":[],"tickets":[],"notes":[],"events":[]}""",
            ),
        )
        assertNull(lonely.mesh)
        assertNull(lonely.deviceName)
        assertEquals("pixel", lonely.hostname)
        assertEquals(Joining("12AB-34CD-56EF", "laptop", null), lonely.joining)
        val offer = lonely.offer!!
        assertEquals("offered", offer.status)
        assertEquals("laptop", offer.by)
        assertEquals(listOf("desktop", "laptop"), offer.devices)
        assertEquals("eris://join/x", offer.ticket)
        assertEquals(1, offer.qr.side)
    }

    @Test
    fun eachRequestSaysWhatItAsksAndWho() {
        val (tasks, watch, server) = state.approvals
        assertEquals("asks to use your database and your files", tasks.asks)
        assertEquals("dev.example.tasks", tasks.who)
        assertTrue(tasks.passphrase)
        assertEquals("asks to join your devices", watch.asks)
        assertTrue(watch.isDevice && watch.viaTicket)
        assertEquals("asks to be your database", server.asks)
        assertEquals("ErisDB", server.replaces.single().second)
        assertEquals("An app with nothing on this phone to vouch for it", server.who)
        assertEquals(
            "Tasks asks to use your database and your files" to "dev.example.tasks wants tasks:read, tasks:write, files:read.",
            tasks.notification(),
        )
        assertEquals(
            "watch asks to join your devices" to "Compare the code 12AB-34CD-56EF with the new device's screen.",
            watch.notification(),
        )
        assertEquals(
            "ErisDB 2 asks to be your database" to "An app with nothing on this phone to vouch for it wants nothing more.",
            server.notification(),
        )
    }

    @Test
    fun approvingSendsTheChosenGrantsOfEachCapabilityInTheOrderAskedAndThePassphrase() {
        val (tasks, watch, server) = state.approvals
        assertEquals(3, tasks.asked.size)
        val all = tasks.asked.toSet()
        val decide = tasks.decide(all, "correct horse")
        assertEquals("decide", decide.getString("op"))
        assertEquals("a1", decide.getString("id"))
        val grants = decide.getJSONObject("grants")
        assertEquals(listOf("tasks:read", "tasks:write"), List(2) { grants.getJSONArray("database/1").getString(it) })
        assertEquals("correct horse", decide.getString("passphrase"))
        // A capability whose grants are all unticked is left out.
        val some = tasks.decide(setOf("database/1" to "tasks:write"), "x").getJSONObject("grants")
        assertEquals(setOf("database/1"), some.keys().asSequence().toSet())
        assertEquals(1, some.getJSONArray("database/1").length())
        val device = watch.decide(emptySet(), "")
        assertFalse("a device is approved as it asked", device.has("grants"))
        assertFalse(device.has("passphrase"))
        assertTrue("a server asking for no grants is approved as it asked", server.decide(emptySet(), "").has("grants"))
    }

    @Test
    fun aJoinTicketSaysWhichDeviceItAdds() {
        val payload = Base64.getUrlEncoder().withoutPadding().encodeToString("""{"v":2,"name":"mac","eid":"e","token":"t"}""".toByteArray())
        assertEquals("mac", joinTicketName("eris://join/$payload"))
        assertNull(joinTicketName("eris://pair/$payload"))
        assertNull(joinTicketName("eris://join/!!!"))
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
    fun aTicketComesWithItsQrCodeLaidOutInModulesAndForADeviceACode() {
        val opened = Opened.parse(
            JSONObject("""{"id":"t1","ticket":"eris://pair/x","code":"K7QX-M2PA","qr":{"side":3,"modules":"010111000"},"expires":600}"""),
        )
        assertEquals("eris://pair/x", opened.ticket)
        assertEquals("K7QX-M2PA", opened.code)
        assertEquals(3, opened.qr.side)
        assertTrue(opened.qr.dark(1, 0))
        assertFalse(opened.qr.dark(0, 0))
        assertTrue(opened.qr.dark(2, 1))
        assertFalse(opened.qr.dark(2, 2))
        assertNull(Opened.parse(JSONObject("""{"id":"t2","ticket":"eris://pair/y","qr":{"side":1,"modules":"1"},"expires":1}""")).code)
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
        assertEquals("a, b and c", listed(listOf("a", "b", "c")))
    }

    @Test
    fun refusalsSayWhatWentWrongInWords() {
        assertNull(refusal(JSONObject("""{"ok":true}""")))
        assertEquals("This takes a passphrase.", refusal(JSONObject("""{"error":"needs_passphrase","message":"x"}""")))
        assertEquals("That is not the passphrase.", refusal(JSONObject("""{"error":"wrong_passphrase"}""")))
        assertEquals(
            "Too many wrong passphrases from this phone. Try again in 1:00.",
            refusal(JSONObject("""{"error":"locked","retry_after":60,"message":"x"}""")),
        )
        assertEquals("Approve only what was asked for.", refusal(JSONObject("""{"error":"not_asked"}""")))
        assertEquals(
            "nothing in this mesh serves database/1 yet",
            refusal(JSONObject("""{"error":"no_server","message":"nothing in this mesh serves database/1 yet"}""")),
        )
        assertEquals("That is not the current passphrase.", passphraseProblem(JSONObject("""{"error":"wrong_passphrase"}""")))
        assertNull(passphraseProblem(JSONObject("""{"ok":true}""")))
        assertEquals("At least 8 characters.", newPassphraseProblem("short", ""))
        assertEquals("The two differ.", newPassphraseProblem("long enough", "long enougj"))
        assertNull(newPassphraseProblem("long enough", "long enough"))
    }
}

package dev.eris.auth

import android.app.Service
import android.content.Intent
import android.os.IBinder
import dev.eris.auth.client.Wire
import org.json.JSONObject

/**
 * Where apps ask ErisAuth, bound through `dev.eris.auth.SIGN_IN`. Android says which app is
 * calling; the daemon checks that the app redeeming each ticket is the one that asked, and the
 * person answers on the page. An app that goes away withdraws its request.
 */
class AuthService : Service() {
    override fun onCreate() {
        super.onCreate()
        Notifications.channel(this)
        Native.ensureStarted(this)
    }

    override fun onBind(intent: Intent): IBinder = binder

    /** Which app opened each request, so only that app can withdraw it. */
    private val askers = java.util.concurrent.ConcurrentHashMap<String, Int>()

    private val binder = object : Wire.Service() {
        override fun onAsk(ask: String, callback: IBinder, uid: Int, pid: Int) {
            val caller = Caller.of(this@AuthService, uid, pid)
            Thread { follow(ask, caller, uid, callback) }.start()
        }

        override fun onWithdraw(id: String, uid: Int) {
            if (askers[id] == uid) Thread { Native.withdraw(id) }.start()
        }
    }

    private fun follow(ask: String, caller: Caller, uid: Int, callback: IBinder) {
        val opened = JSONObject(Native.ask(ask, caller.json()))
        if (opened.has("error")) {
            runCatching { Wire.answer(callback, opened.toString()) }
            return
        }
        val id = opened.getString("id")
        askers[id] = uid
        val gone = IBinder.DeathRecipient { Native.withdraw(id) }
        runCatching { callback.linkToDeath(gone, 0) }
        runCatching { Wire.ticket(callback, id, opened.getString("ticket")) }
        Thread {
            val asked = JSONObject(Native.asked(id, 10 * 60 * 1000L))
            if (!asked.has("settled")) Notifications.asked(this, asked)
        }.start()
        val answer = Native.answer(id)
        runCatching { callback.unlinkToDeath(gone, 0) }
        askers.remove(id)
        Notifications.done(this, id)
        runCatching { Wire.answer(callback, answer) }
    }
}

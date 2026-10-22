package dev.eris.eon

import android.app.Service
import android.content.Intent
import android.os.IBinder
import dev.eris.eon.client.Wire
import kotlin.concurrent.thread

/**
 * Where apps ask ErisOverlayNetwork, bound through `dev.eris.eon.SIGN_IN`. Android says which app is
 * calling; the request reaches the daemon as the app's line on the agent socket would, with the
 * calling package and its signing certificate as who asks, and the person answers on ErisOverlayNetwork's
 * screens. Each request is known by the asking uid and the app's own id for it, so only the app
 * that asked can withdraw it; an app that goes away withdraws its requests.
 */
class AuthService : Service() {
    override fun onCreate() {
        super.onCreate()
        Notifications.channels(this)
        Watcher.start(this)
    }

    override fun onBind(intent: Intent): IBinder = binder

    private val binder = object : Wire.Service() {
        override fun onAsk(requestId: String, requestJson: String, callback: IBinder, uid: Int, pid: Int) {
            val caller = Caller.of(this@AuthService, uid, pid)
            thread(name = "eon-request") { follow("$uid/$requestId", requestJson, caller, callback) }
        }

        override fun onWithdraw(requestId: String, uid: Int) {
            Native.withdraw("$uid/$requestId")
        }
    }

    private fun follow(key: String, line: String, caller: Caller, callback: IBinder) {
        val gone = IBinder.DeathRecipient { Native.withdraw(key) }
        // A callback already dead has no one to answer.
        if (runCatching { callback.linkToDeath(gone, 0) }.isFailure) return
        val answer = Native.ensureStarted(this).fold(
            onSuccess = { Native.request(key, line, caller.json()) },
            onFailure = { org.json.JSONObject().put("error", "ErisOverlayNetwork did not start: ${it.message}").toString() },
        )
        runCatching { callback.unlinkToDeath(gone, 0) }
        runCatching { Wire.answer(callback, answer) }
    }
}

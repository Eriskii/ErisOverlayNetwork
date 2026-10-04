package dev.eris.auth

import android.content.Context
import java.io.File
import org.json.JSONObject

/** ErisAuth's daemon, in this process. Every call answers JSON and blocks, so call off the main thread. */
object Native {
    init {
        System.loadLibrary("erisauth_android")
    }

    external fun start(data: String, runtime: String): String
    external fun command(message: String): String
    external fun ask(ask: String, caller: String): String
    external fun answer(id: String): String
    external fun asked(id: String, withinMs: Long): String
    external fun withdraw(id: String)

    @Volatile private var started = false

    /** Start the daemon once per process, or say why it did not start. */
    @Synchronized
    fun ensureStarted(context: Context): Result<Unit> {
        if (started) return Result.success(Unit)
        val answer = JSONObject(start(File(context.filesDir, "erisauth").path, context.cacheDir.path))
        if (answer.has("error")) return Result.failure(IllegalStateException(answer.getString("error")))
        started = true
        return Result.success(Unit)
    }

    /** One command from the app's screens, as the daemon's socket takes it. */
    fun run(message: JSONObject): JSONObject = JSONObject(command(message.toString()))
}

package dev.eris.eon

import android.content.Context
import java.io.File
import org.json.JSONObject

/**
 * ErisOverlayNetwork's daemon, in this process. `command` and `request` block until they have an answer,
 * so they are called off the main thread.
 */
object Native {
    init {
        System.loadLibrary("eon_android")
    }

    /** Start the daemon with its key and mesh in [data]: `{"ok":true}` or `{"error","message"}`. */
    external fun start(data: String): String

    /** One command from the app's screens, as the daemon's socket takes it. */
    external fun command(message: String): String

    /**
     * An app's request [line], exactly as an app sends it on the agent socket, from [caller] (its
     * package, certificate and pid, as JSON). Answers the agent socket's reply line; [key] takes
     * the request back with [withdraw].
     */
    external fun request(key: String, line: String, caller: String): String

    external fun withdraw(key: String)

    @Volatile private var started = false

    /** Start the daemon once per process, or say why it did not start. */
    @Synchronized
    fun ensureStarted(context: Context): Result<Unit> {
        if (started) return Result.success(Unit)
        val answer = JSONObject(start(File(context.filesDir, "eon").path))
        if (answer.has("error")) return Result.failure(IllegalStateException(answer.optString("message", answer.optString("error"))))
        started = true
        return Result.success(Unit)
    }

    /** One command, as JSON both ways. */
    fun run(message: JSONObject): JSONObject = JSONObject(command(message.toString()))
}

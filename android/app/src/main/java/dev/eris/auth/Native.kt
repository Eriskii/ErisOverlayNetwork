package dev.eris.auth

import android.content.Context
import java.io.File
import org.json.JSONObject

/** ErisAuth's daemon, in this process. Every call answers JSON; `answer` and `asked` block. */
object Native {
    init {
        System.loadLibrary("erisauth_android")
    }

    external fun start(data: String, runtime: String): String
    external fun ask(ask: String, caller: String): String
    external fun answer(id: String): String
    external fun asked(id: String, withinMs: Long): String
    external fun withdraw(id: String)

    @Volatile private var page: String? = null

    /** Start the daemon once per process: the approval page's link, or why it did not start. */
    @Synchronized
    fun ensureStarted(context: Context): Result<String> {
        page?.let { return Result.success(it) }
        val answer = JSONObject(start(File(context.filesDir, "erisauth").path, context.cacheDir.path))
        if (answer.has("error")) return Result.failure(IllegalStateException(answer.getString("error")))
        return Result.success(answer.getString("page").also { page = it })
    }
}

package dev.verter.jetbrains

import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.extensions.PluginId

/**
 * Skeleton health action (JBT1): the only user-visible behavior of the plugin
 * skeleton is proving that it loaded against the pinned platform build.
 *
 * It is intentionally free of semantic work: the sole semantic entry for editor
 * clients remains the native verter-lsp server, and no feature may be added
 * here before JBT2 owns the plugin lifecycle.
 */
class VerterHealthAction : AnAction() {
    override fun actionPerformed(event: AnActionEvent) {
        NotificationGroupManager.getInstance()
            .getNotificationGroup(NOTIFICATION_GROUP)
            .createNotification(healthMessage(), NotificationType.INFORMATION)
            .notify(event.project)
    }

    companion object {
        val PLUGIN_ID: PluginId = PluginId.getId("com.verter.jetbrains")
        const val ACTION_ID: String = "verter.Health"
        const val NOTIFICATION_GROUP: String = "VerterHealth"
        const val NO_SEMANTIC_FEATURES: String = "skeleton: no semantic features yet"

        /**
         * The health payload: installed plugin version plus the running
         * platform build. Anything richer (engine presence, semantic readiness)
         * belongs to the JBT2 lifecycle work, not to the skeleton.
         */
        fun healthMessage(): String {
            val pluginVersion = installedPluginVersion() ?: "unknown"
            val build = ApplicationInfo.getInstance().build.asString()
            return "Verter $pluginVersion loaded on $build ($NO_SEMANTIC_FEATURES)"
        }

        /**
         * Reads the INSTALLED plugin descriptor from this plugin's own
         * classloader: the `<version>` element is patched in at build time, so
         * this reports what the running IDE actually loaded. Deliberately
         * public-API only — the plugin-manager internals that hand out
         * descriptors are not on a plugin's compile classpath in 2026.2.
         */
        fun installedPluginVersion(): String? {
            val descriptor = VerterHealthAction::class.java.classLoader
                .getResourceAsStream("META-INF/plugin.xml")
                ?.bufferedReader()
                ?.use { it.readText() }
                ?: return null
            return Regex("""<version>\s*([^<]+?)\s*</version>""")
                .find(descriptor)
                ?.groupValues
                ?.get(1)
        }
    }
}

package dev.verter.jetbrains

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Guards the plugin descriptor against silent drift between registration and
 * implementation: the action id, class and notification group wired into
 * [VerterHealthAction] must be exactly what plugin.xml declares. Runs without
 * booting a platform instance.
 */
class PluginDescriptorTest {
    @Test
    fun descriptorRegistersHealthActionUnderItsOwnId() {
        val descriptor = javaClass.classLoader.getResourceAsStream("META-INF/plugin.xml")!!
            .readBytes()
            .decodeToString()

        assertTrue(
            "plugin.xml must declare the plugin id",
            Regex("""<id>\s*${Regex.escape(VerterHealthAction.PLUGIN_ID.idString)}\s*</id>""").containsMatchIn(descriptor),
        )
        assertTrue(
            "plugin.xml must register the health action id",
            Regex("""<action[^>]*id="${Regex.escape(VerterHealthAction.ACTION_ID)}"""").containsMatchIn(descriptor),
        )
        assertTrue(
            "plugin.xml must bind the health action id to its implementing class",
            Regex("""<action[^>]*class="${Regex.escape(VerterHealthAction::class.qualifiedName!!)}"""").containsMatchIn(descriptor),
        )
    }

    @Test
    fun descriptorRegistersTheHealthNotificationGroup() {
        val descriptor = javaClass.classLoader.getResourceAsStream("META-INF/plugin.xml")!!
            .readBytes()
            .decodeToString()

        assertTrue(
            "plugin.xml must register the notification group the health action notifies",
            Regex("""<notificationGroup[^>]*id="${Regex.escape(VerterHealthAction.NOTIFICATION_GROUP)}"""").containsMatchIn(descriptor),
        )
    }

    @Test
    fun descriptorDeclaresNoSemanticDependencies() {
        val descriptor = javaClass.classLoader.getResourceAsStream("META-INF/plugin.xml")!!
            .readBytes()
            .decodeToString()

        // The skeleton must not depend on any language plugin or register a
        // second semantic engine: platform module only (JBT0 constitution).
        val depends = Regex("""<depends>([^<]+)</depends>""").findAll(descriptor).map { it.groupValues[1] }.toList()
        assertEquals(listOf("com.intellij.modules.platform"), depends)
    }
}

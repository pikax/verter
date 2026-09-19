package dev.verter.jetbrains

import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Real JVM test against a booted test IDE from the pinned platform build: the
 * health action is the JBT1 load proof, so its registration and payload are
 * asserted on the live ActionManager, not on the static descriptor alone.
 */
class VerterHealthActionTest : BasePlatformTestCase() {
    fun `test health action is registered under its action id`() {
        val action = ActionManager.getInstance().getAction(VerterHealthAction.ACTION_ID)
        assertNotNull("expected `${VerterHealthAction.ACTION_ID}` in the ActionManager", action)
        assertTrue(
            "the registered action must be the Verter health action",
            action is VerterHealthAction,
        )
    }

    fun `test health message reports plugin version and platform build`() {
        val message = VerterHealthAction.healthMessage()
        val pluginVersion = VerterHealthAction.installedPluginVersion()
        assertNotNull(
            "installed plugin version must come from the patched descriptor (was: $message)",
            pluginVersion,
        )
        assertTrue(
            "health message must carry the installed plugin version (was: $message)",
            message.contains(pluginVersion!!),
        )
        assertTrue(
            "health message must carry the running platform build (was: $message)",
            message.contains(com.intellij.openapi.application.ApplicationInfo.getInstance().build.asString()),
        )
        assertTrue(
            "skeleton must stay explicit about carrying no semantic features (was: $message)",
            message.contains(VerterHealthAction.NO_SEMANTIC_FEATURES),
        )
    }
}

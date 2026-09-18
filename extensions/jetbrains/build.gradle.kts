import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType
import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel

plugins {
    id("java")
    kotlin("jvm") version "2.4.20"
    id("org.jetbrains.intellij.platform") version "2.19.0"
}

val platformType = providers.gradleProperty("platformType")
val platformVersion = providers.gradleProperty("platformVersion")

// The pinned product is WebStorm (JBT0 comparator); a drifted platformType
// property fails the build instead of silently pinning a different IDE.
check(platformType.get() == IntelliJPlatformType.WebStorm.code) {
    "platformType must stay pinned to ${IntelliJPlatformType.WebStorm.code} (WebStorm), was ${platformType.get()}"
}

group = providers.gradleProperty("pluginGroup").get()
version = providers.gradleProperty("pluginVersion").get()

kotlin {
    jvmToolchain(21)
}

java {
    // The pinned 2026.2 platform targets JDK 21 class files.
    toolchain {
        languageVersion = JavaLanguageVersion.of(21)
    }
}

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        // JBT1 pin: official WebStorm 2026.2.3 (build 262.10968.77) as declared
        // in tests/jetbrains-product/JBT1 and consumed from JBT0's baseline.
        create(platformType, platformVersion)
        testFramework(TestFrameworkType.Platform)
    }
    testImplementation("junit:junit:4.13.2")
}

intellijPlatform {
    pluginConfiguration {
        id = providers.gradleProperty("pluginId")
        name = providers.gradleProperty("pluginName")
        version = providers.gradleProperty("pluginVersion")
        ideaVersion {
            sinceBuild = providers.gradleProperty("pluginSinceBuild")
            untilBuild = providers.gradleProperty("pluginUntilBuild")
        }
    }
    pluginVerification {
        // AC3: the verifier runs against the pinned build for the declared
        // range; `recommended()` is deliberately not used so verification never
        // depends on whatever IDE builds are latest at run time. Failure is
        // pinned to every level so a default change can never widen what the
        // jetbrains gate accepts.
        failureLevel.set(FailureLevel.ALL)
        ides {
            create(IntelliJPlatformType.WebStorm, platformVersion.get())
        }
    }
}

tasks {
    test {
        useJUnit()
        // The platform test framework boots a real test IDE; give it room.
        maxHeapSize = "2g"
        jvmArgs("-Xss4m")
    }
}

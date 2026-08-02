pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

// Lets the toolchain in `app/build.gradle.kts` fetch the JDK it wants rather
// than depend on whichever one happens to be installed. The system JDK moved to
// 26 on 2026-07-21 and the build stopped compiling — Kotlin 2.3.21's bundled
// version parser rejects "26.0.2" outright, with `IllegalArgumentException:
// 26.0.2` and no other clue as to what is wrong. A build that only works on one
// week's system packages is not a build, so the version it needs is now stated
// rather than assumed.
plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "0.10.0"
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "LOOP_SLCR"
include(":app")

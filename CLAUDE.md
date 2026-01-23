# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Tasks is an open-source Android to-do list application based on the discontinued Astrid app. It supports multiple sync backends including CalDAV, Google Tasks, EteSync, and Microsoft To-Do.

## Build Commands

```bash
# Build debug APK (googleplay flavor - default)
./gradlew app:assembleGoogleplayDebug

# Build debug APK (generic/F-Droid flavor)
./gradlew app:assembleGenericDebug

# Run unit tests for a specific flavor
./gradlew app:testGoogleplayDebugUnitTest
./gradlew app:testGenericDebugUnitTest

# Run a single unit test class
./gradlew app:testGoogleplayDebugUnitTest --tests "com.todoroo.astrid.repeats.RepeatTests"

# Run Android instrumented tests (requires emulator/device)
./gradlew app:connectedGoogleplayDebugAndroidTest
./gradlew app:connectedGenericDebugAndroidTest

# Lint checks
./gradlew :app:lintGoogleplayRelease :app:lintGenericRelease --no-configuration-cache

# Build release bundle (requires signing keys)
./gradlew app:bundleGoogleplayRelease wear:bundleRelease

# Run desktop app (requires JDK 21)
JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew :desktop:runApp --no-configuration-cache
```

## Development Setup

- Requires Android Studio Canary build (uses bleeding-edge features)
- JDK 21 required for building
- Optional API keys in `gradle.properties`:
  - `tasks_mapbox_key_debug` - Mapbox for location features
  - `tasks_google_key_debug` - Google Maps/Places
  - Google Cloud credentials for Google Tasks/Drive sync

## Architecture

### Multi-Module Structure

- **app**: Main Android application with UI and business logic
- **data**: Kotlin Multiplatform module containing Room database entities, DAOs, and data models (shared between Android and JVM)
- **kmp**: Kotlin Multiplatform module with shared Compose UI components
- **desktop**: Compose for Desktop application (Linux, macOS, Windows)
- **wear**: Wear OS companion app
- **wear-datalayer**: Data layer for Wear OS communication
- **icons**: Icon resources module

### Build Flavors

- **googleplay**: Full-featured version with Google Play Services, Firebase, billing, Microsoft auth
- **generic**: F-Droid compatible version without proprietary dependencies

### Key Technologies

- **UI**: Jetpack Compose + traditional Views (migration in progress)
- **DI**: Dagger Hilt
- **Database**: Room with Kotlin Multiplatform support
- **Sync**: CalDAV (dav4jvm), Google Tasks API, EteSync, OpenTasks provider
- **Networking**: OkHttp, Ktor
- **Background**: WorkManager

### Package Structure (app module)

- `com.todoroo.astrid`: Legacy Astrid code (tasks, repeats, subtasks, adapters)
- `org.tasks`: Main application code
  - `caldav/`: CalDAV sync implementation
  - `gtasks/`: Google Tasks sync
  - `sync/`: Microsoft and other sync providers
  - `compose/`: Compose UI components
  - `data/`: Data layer extensions and DAOs
  - `billing/`: In-app purchases (googleplay only)
  - `notifications/`: Notification handling
  - `widget/`: Home screen widgets

### Database

Room database with migrations managed in `data/schemas/`. DAOs are in `data/src/commonMain/kotlin/org/tasks/data/dao/`.

## Testing

Unit tests are in `app/src/test/` and use JUnit with Mockito. Instrumented tests are in `app/src/androidTest/` and require Hilt test infrastructure via `InjectingTestCase`.

Test makers (factory patterns) are in `app/src/test/java/org/tasks/makers/` for creating test data.

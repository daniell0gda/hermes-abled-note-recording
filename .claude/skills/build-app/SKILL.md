---
name: build-app
description: Builds ListeningApp.exe with PyInstaller into .gen\dist, mirrors it into dist\ListeningApp and checks that the bundled assets match the source, refusing while the app runs. Use when the user asks to build the app, rebuild the exe, or wants to test a change in the real app.
---

# Build app

## Quick start

```powershell
& X:\projekty\transcript-app\.claude\skills\build-app\scripts\build-app.ps1
```

It takes about 5 minutes; run it with a long timeout or in the background.

## What the script does

1. Stops with an error when `ListeningApp.exe` runs: the user often keeps it open and it may be recording a
   meeting. Ask the user to close it; never stop the process yourself.
2. Runs `build.ps1 -DistRoot .gen\dist`, so PyInstaller never deletes `dist\ListeningApp` itself
   (deleting it fails when a shell has it as working folder, and the shell here starts there).
3. Mirrors `.gen\dist\ListeningApp` into `dist\ListeningApp` with `robocopy /MIR`.
4. Compares every file in `listening_app\assets` with its bundled copy and fails on any difference.

## Rules

- The mirror step deletes files in `dist\ListeningApp` that the new build no longer has. Ask the user before
  the first run in a session.
- Do not start the exe after the build: the user tests it themselves.
- Report the exe path and whether the build is committed; a build of uncommitted code is worth saying.
- For a screenshot of the running app, capture only its window, never the full screen: the desktop shows the
  user's private chats.

const assert = require("node:assert/strict")
const { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } = require("node:fs")
const { tmpdir } = require("node:os")
const { join, resolve } = require("node:path")
const { test } = require("node:test")

const debugModulePath = resolve(__dirname, "../dist/Debug.js")
const settingsModulePath = resolve(__dirname, "../dist/Settings.js")

test("initializes diagnostics in the user data directory, not the install directory", () => {
    const originalCwd = process.cwd()
    const tempRoot = mkdtempSync(join(tmpdir(), "lyrics-status-startup-"))
    const installDirectory = join(tempRoot, "Program Files", "LyricsStatus")
    const userDataDirectory = join(tempRoot, "user-data", "LyricsStatus")
    mkdirSync(installDirectory, { recursive: true })

    try {
        process.chdir(installDirectory)
        const { Debug } = require(debugModulePath)
        const { initSettingsPath } = require(settingsModulePath)

        initSettingsPath(userDataDirectory)

        const logPath = join(userDataDirectory, "log.txt")
        assert.ok(existsSync(logPath), "startup diagnostics should be written under user data")
        assert.match(readFileSync(logPath, "utf8"), /Settings path initialized:/)
        assert.equal(existsSync(join(installDirectory, "log.txt")), false)
        Debug.path = "./"
    } finally {
        process.chdir(originalCwd)
        rmSync(tempRoot, { recursive: true, force: true })
    }
})

test("diagnostic write failures do not abort application startup", () => {
    const tempRoot = mkdtempSync(join(tmpdir(), "lyrics-status-log-error-"))
    const notADirectory = join(tempRoot, "file")
    const { Debug } = require(debugModulePath)
    writeFileSync(notADirectory, "not a directory")
    const previousPath = Debug.path

    try {
        Debug.path = notADirectory
        assert.doesNotThrow(() => Debug.write("startup should continue"))
    } finally {
        Debug.path = previousPath
        rmSync(tempRoot, { recursive: true, force: true })
    }
})

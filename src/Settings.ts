import { readFileSync, writeFileSync, existsSync, mkdirSync, renameSync, unlinkSync } from "node:fs"
import { join } from "node:path"
import { Debug } from "./Debug"

// Debounce timer for batching rapid saves
let saveDebounceTimer: NodeJS.Timeout | null = null
const SAVE_DEBOUNCE_MS = 500

// Get the settings path - use app.getPath in Electron, fallback to local for CLI
let settingsPath = "./settings.json"

export function initSettingsPath(userDataPath: string): void {
    // Ensure directory exists
    if (!existsSync(userDataPath)) {
        mkdirSync(userDataPath, { recursive: true })
    }
    Debug.path = userDataPath
    settingsPath = join(userDataPath, "settings.json")
    Debug.write(`Settings path initialized: ${settingsPath}`)
}

export function getSettingsPath(): string {
    return settingsPath
}

export interface IAllSettings {
    credentials: typeof Settings.credentials
    view: typeof Settings.view
    timings: typeof Settings.timings
    general: typeof Settings.general
    appearance: typeof Settings.appearance
    update: typeof Settings.update
}

export class Settings {
    public static credentials = {
        token: "",
        cookies: "",
        clientID: "",
        clientSecret: "",
        useExternalAuthServer: false,
        code: "",
        refreshToken: "",
        uuid: "",
        customRedirectUri: "http://127.0.0.1:67/callback"
    }

    public static view = {
        timestamp: true,
        label: true,
        emoji: {
            enabled: false,
            name: "🎶",
            id: null as string | null,
            animated: false
        },
        advanced: {
            enabled: false,
            customStatus: "[{timestamp}] {lyrics}"
        },
        behavior: {
            mode: "single" as "single" | "multi",
            transition: "fade" as "fade" | "slide-up" | "slide-down" | "blur" | "scale" | "none",
            alignment: "center" as "left" | "center" | "right",
            maxLength: 128
        }
    }

    public static timings = {
        sendTimeOffset: 500,
        enableAutooffset: true,
        autooffset: 3
    }

    public static general = {
        autoStart: false,
        startMinimized: false,
        theme: "oled" as "dark" | "light" | "spotify" | "discord" | "midnight" | "obsidian" | "oled" | "frost",
        language: "en" as "en" | "de"
    }

    public static appearance = {
        theme: "oled" as "midnight" | "obsidian" | "discord" | "oled" | "frost",
        accentSource: "spotify" as "spotify" | "discord" | "artwork" | "custom",
        accentColor: "#1DB954",
        backgroundStyle: "solid" as "solid" | "gradient" | "artwork-blur" | "banner-blur",
        backgroundBlur: 40,
        backgroundOpacity: 80,
        backgroundAnimation: "none" as "none" | "aurora" | "mesh" | "orbs" | "particles" | "grain" | "waves" | "grid" | "rain" | "shimmer" | "liquid",
        animationIntensity: 60,
        animationSpeed: 1.2
    }

    public static update = {
        enableAutoupdate: true
    }

    public static save(): void {
        // Debounce to batch rapid saves
        if (saveDebounceTimer) {
            clearTimeout(saveDebounceTimer)
        }
        
        saveDebounceTimer = setTimeout(() => {
            this.saveImmediate()
        }, SAVE_DEBOUNCE_MS)
    }

    public static saveImmediate(): void {
        const tempPath = settingsPath + '.tmp'
        
        try {
            // Write to temp file first
            writeFileSync(tempPath, JSON.stringify({
                credentials: this.credentials,
                view: this.view,
                timings: this.timings,
                general: this.general,
                appearance: this.appearance,
                update: this.update
            }, null, 2))
            
            // Atomic rename
            renameSync(tempPath, settingsPath)
            Debug.write(`Settings saved atomically to: ${settingsPath}`)
        } catch (e) {
            Debug.write(`Failed to save settings: ${(e as Error).message}`)
            // Clean up temp file if it exists
            try {
                if (existsSync(tempPath)) unlinkSync(tempPath)
            } catch {}
        }
    }

    public static load(): void {
        Debug.write(`Loading settings from: ${settingsPath}`)
        if (!existsSync(settingsPath)) {
            Debug.write("Settings file not found, creating with defaults")
            this.save()
            return
        }

        let settings

        try {
            settings = JSON.parse(readFileSync(settingsPath).toString())
            Debug.write("Settings loaded successfully")
        } catch(e) {
            Debug.write("An error occurred while trying to read settings from file. Using defaults. Error: " + (e as Error).stack)
        }

        if (settings) {
            this.credentials = { ...this.credentials, ...settings.credentials }
            this.view = { ...this.view, ...settings.view }
            this.timings = { ...this.timings, ...settings.timings }
            this.general = { ...this.general, ...settings.general }
            this.appearance = { ...this.appearance, ...settings.appearance }
            this.update = { ...this.update, ...settings.update }
        }
    }

    public static getAll() {
        return {
            credentials: this.credentials,
            view: this.view,
            timings: this.timings,
            general: this.general,
            appearance: this.appearance,
            update: this.update
        }
    }

    public static setAll(settings: Partial<ReturnType<typeof Settings.getAll>>): void {
        if (settings.credentials) this.credentials = { ...this.credentials, ...settings.credentials }
        if (settings.view) this.view = { ...this.view, ...settings.view }
        if (settings.timings) this.timings = { ...this.timings, ...settings.timings }
        if (settings.general) this.general = { ...this.general, ...settings.general }
        if (settings.appearance) this.appearance = { ...this.appearance, ...settings.appearance }
        if (settings.update) this.update = { ...this.update, ...settings.update }
    }
}

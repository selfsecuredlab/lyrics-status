import { PlaybackState } from "./PlaybackState"
import { Settings } from "./Settings"
import { LyricsLine } from "./Sources/BaseSource"
import { Autooffset } from "./Autooffset"
import { serviceEvents } from "./ServiceEvents"

interface DiscordSettingsResponse {
    custom_status?: Record<string, unknown> | null
}

export class StatusChanger {
    public playbackState: PlaybackState
    public sentLines: LyricsLine[]
    public sentNoLyricsStatus: boolean
    public lastNoLyricsUpdate: number
    public autooffset: Autooffset
    
    // Track last sent status to avoid duplicates
    private lastSentStatusText: string = ""
    private originalCustomStatus: Record<string, unknown> | null = null
    private originalStatusCaptured: boolean = false
    private lyricsStatusActive: boolean = false
    private pendingStatusRequest: Promise<Response> | null = null

    constructor(playbackState: PlaybackState) {
        this.playbackState = playbackState
        this.sentLines = []
        this.sentNoLyricsStatus = false
        this.lastNoLyricsUpdate = 0
        this.autooffset = new Autooffset()
    }

    private async discordRequest(url: string, init: RequestInit): Promise<Response> {
        const maxAttempts = 3
        let wasRateLimited = false
        for (let attempt = 0; attempt < maxAttempts; attempt++) {
            const response = await fetch(url, init)
            if (response.status !== 429 || attempt === maxAttempts - 1) {
                if (response.ok && wasRateLimited) serviceEvents.emit("discord:rate-limit-cleared", { source: "discord-status" })
                return response
            }
            wasRateLimited = true

            let retryAfterSeconds = Number.parseFloat(response.headers.get("Retry-After") || "")
            let isGlobal = false
            try {
                const body = await response.clone().json() as { retry_after?: number; global?: boolean }
                if (Number.isFinite(body.retry_after)) retryAfterSeconds = Number(body.retry_after)
                isGlobal = body.global === true
            } catch {}

            const retryAfterMs = Number.isFinite(retryAfterSeconds) && retryAfterSeconds > 0
                ? Math.ceil(retryAfterSeconds * 1000)
                : Math.pow(2, attempt) * 1000

            serviceEvents.emit("discord:rate-limit", {
                source: "discord-status",
                retryAfterMs,
                retryAt: Date.now() + retryAfterMs,
                attempt: attempt + 1,
                maxAttempts,
                global: isGlobal
            })
            await new Promise(resolve => setTimeout(resolve, retryAfterMs))
        }

        throw new Error("Discord request retry loop ended unexpectedly")
    }

    public async captureOriginalStatus(): Promise<boolean> {
        if (this.originalStatusCaptured) return true
        if (!Settings.credentials.token) return false

        try {
            const response = await this.discordRequest("https://discord.com/api/v10/users/@me/settings", {
                headers: { "Authorization": Settings.credentials.token }
            })
            if (!response.ok) return false

            const discordSettings = await response.json() as DiscordSettingsResponse
            this.originalCustomStatus = discordSettings.custom_status
                ? { ...discordSettings.custom_status }
                : null
            this.originalStatusCaptured = true
            return true
        } catch {
            return false
        }
    }

    public changeStatusRequest(text: string, token: string, emoji: string | null, emojiId?: string | null, animated?: boolean): Promise<Response> {
        const now = Date.now()
        
        // Skip if this is the same status we just sent
        if (text === this.lastSentStatusText) {
            return Promise.resolve(new Response(null, { status: 304, statusText: "Not Modified" }))
        }
        
        // Build custom_status payload for Discord JSON API
        // Note: Custom/animated emojis have limited support via JSON API
        const customStatus: any = {
            text,
            expires_at: new Date(Date.now() + 600000).toISOString()
        }
        
        // Add emoji - prefer emoji_name for standard emojis
        // JSON API has limited support for custom emojis
        if (emoji) {
            // Standard Unicode emoji - works reliably
            customStatus.emoji_name = emoji
        }
        
        const request = this.discordRequest("https://discord.com/api/v10/users/@me/settings", {
            method: "PATCH",
            headers: {
                "Content-Type": "application/json",
                "Authorization": token
            },
            body: JSON.stringify({
                custom_status: customStatus
            })
        }).then(response => {
            if (!response.ok) {
                console.error(`[Discord API] Error ${response.status}: ${response.statusText}`)
            } else {
                this.lyricsStatusActive = true
            }
            return response
        }).catch(error => {
            console.error(`[Discord API] Status update failed: ${(error as Error).message}`)
            return new Response(null, { status: 503, statusText: "Discord request failed" })
        })

        this.lastSentStatusText = text
        this.pendingStatusRequest = request
        request.then(() => this.autooffset.addValue(Date.now() - now))
            .finally(() => {
                if (this.pendingStatusRequest === request) this.pendingStatusRequest = null
            })
        return request
    }

    public async restoreOriginalStatus(resetCapture: boolean = false): Promise<Response | null> {
        if (!this.originalStatusCaptured || !this.lyricsStatusActive) {
            if (resetCapture) this.originalStatusCaptured = false
            return null
        }

        if (this.pendingStatusRequest) {
            try { await this.pendingStatusRequest } catch {}
        }

        // Claim the restoration before awaiting the network so overlapping
        // playback polls cannot submit the same restoration twice.
        this.lyricsStatusActive = false
        this.lastSentStatusText = ""
        const response = await this.discordRequest("https://discord.com/api/v10/users/@me/settings", {
            method: "PATCH",
            headers: {
                "Content-Type": "application/json",
                "Authorization": Settings.credentials.token
            },
            body: JSON.stringify({ custom_status: this.originalCustomStatus })
        })
        if (!response.ok) this.lyricsStatusActive = true
        if (resetCapture) this.originalStatusCaptured = false
        return response
    }

    // Compatibility alias: clearing now means restoring what the user had before.
    public clearStatus(): Promise<Response | null> {
        return this.restoreOriginalStatus()
    }
    
    // Reset emoji tracking (call when user changes emoji settings)
    public resetEmojiTracking(): void {
        this.lastSentStatusText = ""
    }

    // Check if text contains Chinese characters
    private containsChinese(text: string): boolean {
        return /[\u4e00-\u9fff\u3400-\u4dbf]/.test(text)
    }

    private getMaxStatusLength(): number {
        const requestedLength = Number(Settings.view.behavior?.maxLength ?? 128)
        return Number.isFinite(requestedLength) ? Math.max(32, Math.min(128, Math.round(requestedLength))) : 128
    }

    private getLyricText(line: LyricsLine): string {
        if (Settings.view.behavior?.mode !== "multi") return line.text

        const lines = this.playbackState.lyrics?.lines || []
        const currentIndex = lines.findIndex(candidate => candidate === line || candidate.time === line.time)
        const nextLine = currentIndex >= 0
            ? lines.slice(currentIndex + 1).find(candidate => !!candidate.text && !this.containsChinese(candidate.text))
            : undefined
        return nextLine ? `${line.text} • ${nextLine.text}` : line.text
    }

    public changeStatus(): void {
        this.autooffset.setLimit(Settings.timings.autooffset)

        const playbackState = this.playbackState

        if (playbackState.ended || !playbackState.isPlaying) return

        // If no lyrics, show song title instead
        if (!playbackState.hasLyrics) {
            const now = Date.now()
            // Resend every 5 minutes to keep status alive
            const shouldResend = !this.sentNoLyricsStatus || (now - this.lastNoLyricsUpdate > 300000)
            
            if (shouldResend) {
                const songName = playbackState.songName
                const songAuthor = playbackState.songAuthor
                const noLyricsText = `${songName} - ${songAuthor}`
                
                if (Settings.view.advanced.enabled) {
                    // In advanced mode, use custom format with song name & artist instead of lyrics
                    const statusText = Settings.view.advanced.customStatus
                        .replace("{lyrics}", noLyricsText)
                        .replace("{lyrics_upper}", noLyricsText.toUpperCase())
                        .replace("{lyrics_lower}", noLyricsText.toLowerCase())
                        .replace("{lyrics_letters_only}", noLyricsText.replace(/['",\.]/gi, ""))
                        .replace("{timestamp}", "0:00")
                        .replace("{elapsed}", this.formatSeconds(+(playbackState.songProgress / 1000).toFixed(0)))
                        .replace("{remaining}", this.formatSeconds(+(Math.max(0, playbackState.songDuration - playbackState.songProgress) / 1000).toFixed(0)))
                        .replace("{duration}", this.formatSeconds(+(playbackState.songDuration / 1000).toFixed(0)))
                        .replace("{song}", songName)
                        .replace("{song_name}", songName)
                        .replace("{artist}", songAuthor)
                        .replace("{song_author}", songAuthor)
                        .replace("{album}", playbackState.songAlbum)
                        .slice(0, this.getMaxStatusLength())
                    // In advanced mode, don't add emoji - user's custom format has their own emojis
                    this.changeStatusRequest(statusText, Settings.credentials.token, null)
                } else {
                    const statusText = noLyricsText.slice(0, this.getMaxStatusLength())
                    this.changeStatusRequest(statusText, Settings.credentials.token, null)
                }
                this.sentNoLyricsStatus = true
                this.lastNoLyricsUpdate = now
            }
            return
        }

        this.sentNoLyricsStatus = false
        const lyrics = playbackState.lyrics

        if (!lyrics) return

        const currentLine = playbackState.currentLine
        const songProgress = playbackState.songProgress
        const lines = lyrics.lines
        const offset = Settings.timings.enableAutooffset ? this.autooffset.getAverageValue() + 100 : Settings.timings.sendTimeOffset

        // Find the most recent line that should be displayed
        let lineToDisplay: LyricsLine | null = null
        
        for (let i = 0; i < lines.length; i++) {
            const line = lines[i]
            const nextLine = lines[i + 1]

            // Check if this line's time has passed (with offset)
            if (line.time < (songProgress + offset)) {
                if (!line.text) continue
                // Skip lines with Chinese characters (wrong lyrics source)
                if (this.containsChinese(line.text)) continue
                // Skip if next line has also passed (we want the most recent active line)
                if (nextLine && nextLine.time < (songProgress + offset)) continue
                
                // This is the line that should currently be displayed
                lineToDisplay = line
            }
        }
        
        // If we found a line to display and it's different from current
        if (lineToDisplay && lineToDisplay !== currentLine) {
            // Check if this specific line was already sent
            if (!this.sentLines.some((sentLine) => sentLine.time === lineToDisplay!.time)) {
                playbackState.currentLine = lineToDisplay

                if (Settings.view.advanced.enabled) {
                    // In advanced mode, don't add emoji_name - user's custom format has their own emojis
                    this.changeStatusRequest(this.parseStatusString(Settings.view.advanced.customStatus), Settings.credentials.token, null)
                } else {
                    const statusText = this.getStatusString(lineToDisplay)
                    this.changeStatusRequest(statusText, Settings.credentials.token, null)
                }

                this.sentLines.push(lineToDisplay)
            }
        }
    }

    public songChanged(): void {
        this.sentLines = []
        this.sentNoLyricsStatus = false
        this.lastNoLyricsUpdate = 0
        this.lastSentStatusText = "" // Reset to allow new song status
    }

    public getCurrentStatusText(): string {
        const currentLine = this.playbackState.currentLine
        if (!currentLine) return ""
        
        if (Settings.view.advanced.enabled) {
            return this.parseStatusString(Settings.view.advanced.customStatus)
        }
        return this.getStatusString(currentLine)
    }

    public formatSeconds(s: number): string {
        return (s - (s %= 60)) / 60 + (9 < s ? ':' : ':0' ) + s
    }

    public getStatusString(line: LyricsLine): string {
        const lyricText = this.getLyricText(line).replace("♪", "🎶")
        return `${Settings.view.timestamp ? `[${this.formatSeconds(+(line.time / 1000).toFixed(0))}] ` : ""}${Settings.view.label ? "Song lyrics - " : ""}${lyricText}`.slice(0, this.getMaxStatusLength())
    }

    public parseStatusString(status: string): string {
        if(this.playbackState.currentLine) {
            const line = this.playbackState.currentLine
            const lyricText = this.getLyricText(line)
            const songName = this.playbackState.songName
            const songAuthor = this.playbackState.songAuthor

            status = status
                .replace("{lyrics}", lyricText)
                .replace("{lyrics_upper}", lyricText.toUpperCase())
                .replace("{lyrics_lower}", lyricText.toLowerCase())
                .replace("{lyrics_letters_only}", lyricText.replace(/['",\.]/gi, ""))
                .replace("{lyrics_upper_letters_only}", lyricText.toUpperCase().replace(/['",\.]/gi, ""))
                .replace("{lyrics_lower_letters_only}", lyricText.toLowerCase().replace(/['",\.]/gi, ""))
                .replace("{timestamp}", this.formatSeconds(+(line.time / 1000).toFixed()))
                .replace("{elapsed}", this.formatSeconds(+(this.playbackState.songProgress / 1000).toFixed(0)))
                .replace("{remaining}", this.formatSeconds(+(Math.max(0, this.playbackState.songDuration - this.playbackState.songProgress) / 1000).toFixed(0)))
                .replace("{duration}", this.formatSeconds(+(this.playbackState.songDuration / 1000).toFixed(0)))
                .replace("{song}", songName)
                .replace("{song_name}", songName)
                .replace("{song_name_upper}", songName.toUpperCase())
                .replace("{song_name_lower}", songName.toLowerCase())
                .replace("{song_name_cropped}", songName.replace(/( ?- ?.+)|(\(.+\))/gi, ""))
                .replace("{song_name_upper_cropped}", songName.toUpperCase().replace(/( ?- ?.+)|(\(.+\))/gi, ""))
                .replace("{song_name_lower_cropped}", songName.toLowerCase().replace(/( ?- ?.+)|(\(.+\))/gi, ""))
                .replace("{artist}", songAuthor)
                .replace("{song_author}", songAuthor)
                .replace("{song_author_upper}", songAuthor.toUpperCase())
                .replace("{song_author_lower}", songAuthor.toLowerCase())
                .replace("{album}", this.playbackState.songAlbum);
        }

        return status.slice(0, this.getMaxStatusLength());
    }
}

import { EventEmitter } from "node:events"

export interface DiscordRateLimitNotice {
    source: "spotify-connection" | "discord-status"
    retryAfterMs: number
    retryAt: number
    attempt: number
    maxAttempts: number
    global: boolean
}

export const serviceEvents = new EventEmitter()


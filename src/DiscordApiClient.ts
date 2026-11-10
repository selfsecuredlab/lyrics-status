import { serviceEvents } from "./ServiceEvents"

export type DiscordRequestSource = "discord-status" | "spotify-connection" | "auth"

export interface DiscordRequestOptions extends RequestInit {
    source: DiscordRequestSource
    maxAttempts?: number
    timeoutMs?: number
}

let globalBlockedUntil = 0

function sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms))
}

async function retryDelay(response: Response, attempt: number): Promise<{ delayMs: number; global: boolean }> {
    let retryAfter = Number.parseFloat(response.headers.get("Retry-After") || "")
    let global = false
    try {
        const body = response.clone() as Response
        // Parsing is intentionally best-effort; Discord also supplies the header.
        const value = await body.json()
        const data = value as { retry_after?: number; global?: boolean }
        if (Number.isFinite(data.retry_after)) retryAfter = Number(data.retry_after)
        global = data.global === true
    } catch {}
    const delayMs = Number.isFinite(retryAfter) && retryAfter > 0
        ? Math.min(120_000, Math.ceil(retryAfter * 1000))
        : Math.min(120_000, 1_000 * Math.pow(2, attempt))
    return { delayMs, global }
}

export async function discordRequest(url: string, options: DiscordRequestOptions): Promise<Response> {
    const { source, maxAttempts = 3, timeoutMs = 15_000, ...init } = options
    let wasRateLimited = false

    for (let attempt = 0; attempt < maxAttempts; attempt++) {
        const waitFor = globalBlockedUntil - Date.now()
        if (waitFor > 0) await sleep(waitFor)

        const controller = new AbortController()
        const timeout = setTimeout(() => controller.abort(), timeoutMs)
        try {
            const response = await fetch(url, { ...init, signal: controller.signal })
            if (response.status !== 429 || attempt === maxAttempts - 1) {
                if (response.ok && wasRateLimited) {
                    serviceEvents.emit("discord:rate-limit-cleared", { source })
                }
                return response
            }

            wasRateLimited = true
            const { delayMs, global } = await retryDelay(response, attempt)
            if (global) globalBlockedUntil = Math.max(globalBlockedUntil, Date.now() + delayMs)
            serviceEvents.emit("discord:rate-limit", {
                source,
                retryAfterMs: delayMs,
                retryAt: Date.now() + delayMs,
                attempt: attempt + 1,
                maxAttempts,
                global
            })
            await sleep(delayMs)
        } finally {
            clearTimeout(timeout)
        }
    }

    throw new Error("Discord request retry loop ended unexpectedly")
}

export function clearDiscordRateLimit(): void {
    globalBlockedUntil = 0
}

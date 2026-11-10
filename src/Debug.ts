import { appendFileSync } from "fs"
import { join } from "path"

export class Debug {
    public static path: string = "./"

    public static write(text: string): void {
        const date = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })

        try {
            appendFileSync(join(this.path, "log.txt"), `[${date}]: ${text}\n`)
        } catch {
            // Diagnostics must never prevent the application from starting.
        }
    }
}

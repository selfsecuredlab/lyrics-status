import express from "express"
import { createServer, Server } from "node:http"
import { WebSocketServer } from "ws"
import { join } from "node:path"
import { Settings } from "../Settings"

let httpServer: Server | null = null
let wss: WebSocketServer | null = null

export function startServer(): void {
    const app = express()
    httpServer = createServer(app)
    wss = new WebSocketServer({
        server: httpServer,
        path: "/ws"
    })

    app.use("/", express.static(join(__dirname, "../../static")))

    app.get("/", (req, res) => {
        res.sendFile(join(__dirname, "../../static/index.html"))
    })

    wss.on("connection", (ws) => {
        ws.on("message", (data) => {
            const settings = JSON.parse(data.toString())
            // Not typed but it's necessary

            Settings.credentials = settings.credentials
            Settings.view = settings.view
            Settings.timings = settings.timings
            Settings.update = settings.update

            Settings.save()
        })

        const settings = JSON.stringify({
            credentials: Settings.credentials,
            view: Settings.view,
            timings: Settings.timings,
            update: Settings.update
        })

        ws.send(settings)
    })

    const panelPort = 67
    const panelHost = "127.0.0.1"

    httpServer.listen(panelPort, panelHost, () => {
        console.log(`Local panel started on http://${panelHost}:${panelPort}`)
    })

    httpServer.on("error", (err: NodeJS.ErrnoException) => {
        if (err.code === "EADDRINUSE") {
            console.error(`Local panel port ${panelPort} is already in use.`)
        } else if (err.code === "EACCES") {
            console.error(`Local panel port ${panelPort} cannot be opened with the current permissions.`)
        } else {
            console.error("Server error:", err)
        }
    })
}

export function stopServer(): void {
    if (wss) {
        // Close all WebSocket connections
        wss.clients.forEach((client) => {
            client.terminate()
        })
        wss.close()
        wss = null
    }
    if (httpServer) {
        // Close all connections and stop listening
        httpServer.closeAllConnections()
        httpServer.close()
        httpServer = null
    }
}

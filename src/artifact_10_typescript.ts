// src/health.ts
import http from "http";

export function startHealthServer(port = 8080) {
  const server = http.createServer((req, res) => {
    if (req.url === "/health" && req.method === "GET") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({
        status: "HEALTHY",
        timestamp: new Date().toISOString(),
        service: "mozaikflow-enterprise-engine",
        uptimeSeconds: process.uptime()
      }));
    } else {
      res.writeHead(404);
      res.end();
    }
  });

  server.listen(port, () => {
    console.log(`[Health Service] Telemetry listening on port ${port}`);
  });
}
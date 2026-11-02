// Embedded into client/src/daemon.ts
import * as http from 'http';

const server = http.createServer((req, res) => {
  if (req.url === '/healthz' && req.method === 'GET') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({
      status: 'UP',
      uptime: process.uptime(),
      timestamp: Date.now(),
      cluster: process.env.SOLANA_RPC_URL || 'devnet'
    }));
    return;
  }
  res.writeHead(404);
  res.end();
});

server.listen(process.env.PORT || 8080, () => {
  console.log(`[Healthcheck Probe] Listening on :${process.env.PORT || 8080}/healthz`);
});
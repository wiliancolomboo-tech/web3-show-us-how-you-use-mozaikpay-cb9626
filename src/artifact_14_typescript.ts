import http from 'http';

const healthPort = process.env.PORT || 8080;
http.createServer((req, res) => {
  if (req.url === '/health') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ status: 'UP', service: 'MozaikPay StreamEngine', timestamp: new Date().toISOString() }));
  } else {
    res.writeHead(404);
    res.end();
  }
}).listen(healthPort, () => {
  logger.info(`Liveness probe listening on port ${healthPort}`);
});